use super::{Error, WorkflowRunner, WorkflowStatus};
use crate::{
    common::RepoMap,
    config::{RepoConfig, ReviewsConfig, StatusFailuresConfig},
    github::{
        Branch, BranchProtection, GithubClient, MergeableState, PullRequest, PullRequestIdentifier,
        PullRequestReview, PullRequestState, ReviewState, StatusState, WorkflowRunConclusion,
    },
};
use async_trait::async_trait;
use log::{info, warn};
use reqwest::Url;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

#[async_trait]
pub trait Step: fmt::Display {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error>;
}

#[derive(Debug)]
pub struct Context {
    pub identifier: PullRequestIdentifier,
    pub pull_request: PullRequest,
}

impl Context {
    pub fn new(identifier: PullRequestIdentifier, pull_request: PullRequest) -> Self {
        Self {
            identifier,
            pull_request,
        }
    }
}

#[derive(PartialEq, Debug, Clone, Hash)]
pub enum StepStatus {
    Passed,
    Waiting,
}

/// Checks whether a pull request is open and in a mergeable state.
#[derive(Default)]
pub struct CheckCurrentStateStep;

#[async_trait]
impl Step for CheckCurrentStateStep {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        match context.pull_request.state {
            PullRequestState::Open => {
                if context.pull_request.draft {
                    Err(Error::as_generic("pull request is a draft"))
                } else if matches!(context.pull_request.mergeable_state, MergeableState::Dirty) {
                    Err(Error::as_generic("pull request has conflicts"))
                } else {
                    Ok(StepStatus::Passed)
                }
            }
            PullRequestState::Closed if context.pull_request.merged => {
                Err(Error::as_generic("pull request is already merged"))
            }
            PullRequestState::Closed => Err(Error::as_generic("pull request is closed")),
            PullRequestState::Unknown => Err(Error::UnsupportedPullRequestState(
                "pull request state is unknown".into(),
            )),
        }
    }
}

impl fmt::Display for CheckCurrentStateStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check current state")
    }
}

/// Checks whether a pull request is approved by however many people its branch protection
/// rules require
pub struct CheckReviewsStep {
    github: Arc<dyn GithubClient>,
    repo_configs: RepoMap<ReviewsConfig>,
}

impl CheckReviewsStep {
    pub fn new(
        github: Arc<dyn GithubClient>,
        default_config: ReviewsConfig,
        repos: &[RepoConfig],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut repo_configs = RepoMap::new(default_config);
        for repo_config in repos {
            if let Some(reviews) = &repo_config.reviews {
                let repo_id = repo_config.repo.parse()?;
                repo_configs.insert(repo_id, reviews.clone())?;
            }
        }
        Ok(Self {
            github,
            repo_configs,
        })
    }

    async fn fetch_branch_protection(
        &self,
        branch: &Branch,
    ) -> Result<Option<BranchProtection>, Error> {
        let branch_protection = self.github.branch_protection(branch).await;
        match branch_protection {
            Ok(branch_protection) => Ok(Some(branch_protection)),
            Err(e) if e.not_found() => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn compute_approvals(reviews: &[PullRequestReview]) -> usize {
        let mut users_approved = HashSet::new();
        for review in reviews {
            match review.state {
                ReviewState::Approved => users_approved.insert(&review.user.login),
                _ => users_approved.remove(&review.user.login),
            };
        }
        users_approved.len()
    }

    fn required_approvals(
        &self,
        branch_protection: Option<BranchProtection>,
        context: &Context,
    ) -> u32 {
        let id = &context.identifier;
        let configured_approvals = self.repo_configs.get(&id.owner, &id.repo).approvals;
        match branch_protection {
            Some(protection) => protection.reviews.approvals.max(configured_approvals),
            None => configured_approvals,
        }
    }
}

#[async_trait]
impl Step for CheckReviewsStep {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        let branch_protection = self
            .fetch_branch_protection(&context.pull_request.base)
            .await?;
        let approvals_needed = self.required_approvals(branch_protection, context) as usize;
        let reviews = self
            .github
            .pull_request_reviews(&context.identifier)
            .await?;
        let total_users_approved = Self::compute_approvals(&reviews);

        if total_users_approved < approvals_needed {
            let reason = format!(
                "not enough approvals (need {}, have {})",
                approvals_needed, total_users_approved
            );
            Err(Error::as_generic(reason))
        } else {
            Ok(StepStatus::Passed)
        }
    }
}

impl fmt::Display for CheckReviewsStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check reviews")
    }
}

/// Checks whether a pull request is behind master, and updates it otherwise
pub struct CheckBehindMaster {
    github: Arc<dyn GithubClient>,
}

impl CheckBehindMaster {
    pub fn new(github: Arc<dyn GithubClient>) -> Self {
        Self { github }
    }
}

#[async_trait]
impl Step for CheckBehindMaster {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        if !matches!(context.pull_request.mergeable_state, MergeableState::Behind) {
            return Ok(StepStatus::Passed);
        }
        let result = self
            .github
            .update_branch(&context.identifier, &context.pull_request.head.sha)
            .await;
        match result {
            Ok(_) => Ok(StepStatus::Waiting),
            // Technically we should retry but this means the head sha has _just_ changed so
            // odds are someone just did it manually which means we're waiting either way
            Err(e) if e.unprocessable_entity() => Ok(StepStatus::Waiting),
            Err(e) => Err(e.into()),
        }
    }
}

impl fmt::Display for CheckBehindMaster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check if behind master")
    }
}

/// Checks whether the build for a pull request failed, re-triggering CI runs if needed
pub struct CheckBuildFailed {
    github: Arc<dyn GithubClient>,
    workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
    repo_configs: RepoMap<HashMap<String, StatusFailuresConfig>>,
    last_head_hash: Option<String>,
    status_failures: HashMap<String, u32>,
}

impl CheckBuildFailed {
    pub fn new(
        github: Arc<dyn GithubClient>,
        workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
        repos: &[RepoConfig],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut repo_configs = RepoMap::default();
        for repo in repos {
            if repo.statuses.is_empty() {
                continue;
            }
            let mut status_config = HashMap::new();
            for status in &repo.statuses {
                // TODO: dedup
                status_config.insert(status.name.clone(), status.failures.clone());
            }
            repo_configs.insert(repo.repo.parse()?, status_config)?;
        }
        Ok(Self {
            github,
            workflow_runners,
            repo_configs,
            last_head_hash: None,
            status_failures: HashMap::default(),
        })
    }

    async fn check_actions(&self, context: &Context) -> Result<StepStatus, Error> {
        let action_runs = self.github.action_runs(&context.pull_request).await?;
        let mut last_run_per_workflow = HashMap::new();
        for run in action_runs.workflow_runs {
            if run.head_sha != context.pull_request.head.sha {
                continue;
            }
            last_run_per_workflow.entry(run.workflow_id).or_insert(run);
        }
        let failed_workflows: Vec<_> = last_run_per_workflow
            .values()
            .filter(|run| run.conclusion == Some(WorkflowRunConclusion::Failure))
            .collect();
        if failed_workflows.is_empty() {
            return Ok(StepStatus::Passed);
        }
        for run in failed_workflows {
            warn!("Actions workflow '{}' failed, re-running it", run.name);
            self.github
                .rerun_workflow(&context.pull_request.base.repo, run.id)
                .await?;
        }
        Ok(StepStatus::Waiting)
    }

    async fn check_statuses(&mut self, context: &Context) -> Result<StepStatus, Error> {
        // TODO: split this bunch of code a bit
        let summaries = self.fetch_status_summaries(context).await?;
        let pending_count = summaries.pending_statuses.len();
        if pending_count == 0 {
            if summaries.failed_statuses.is_empty() {
                return Ok(StepStatus::Passed);
            }
            warn!(
                "Processing {} failed external jobs",
                summaries.failed_statuses.len()
            );
            self.check_max_failures(context, &summaries.failed_statuses)?;
            let failed_job_urls: Vec<_> = summaries
                .failed_statuses
                .into_iter()
                .map(|summary| summary.url)
                .collect();
            let mut total_triggered = 0;
            for runner in &self.workflow_runners {
                if runner.process_failed_jobs(&failed_job_urls).await? == WorkflowStatus::Triggered
                {
                    total_triggered += 1;
                }
            }
            if total_triggered == 0 {
                // There's failed jobs but we don't know how to re-trigger them. e.g. we don't support
                // whatever service they're being ran on.
                return Err(Error::as_generic(
                    "failed jobs belong to unknown external services",
                ));
            }
        } else if pending_count == 1 {
            info!(
                "Waiting for external job '{}' to finish running",
                summaries.pending_statuses[0].name
            );
        } else {
            info!(
                "Waiting for {} external jobs to finish running",
                summaries.pending_statuses.len()
            );
        };
        Ok(StepStatus::Waiting)
    }

    fn check_max_failures(
        &mut self,
        context: &Context,
        failed_statuses: &[StatusSummary],
    ) -> Result<(), Error> {
        let status_configs = self
            .repo_configs
            .get(&context.identifier.owner, &context.identifier.repo);

        for status in failed_statuses {
            if let Some(config) = status_configs.get(&status.name) {
                let failures = self.status_failures.entry(status.name.clone()).or_insert(0);
                *failures += 1;
                if *failures >= config.max_failures {
                    return Err(Error::as_generic(format!(
                        "Status check {} reached {} failures",
                        status.name, failures
                    )));
                }
            }
        }
        Ok(())
    }

    async fn fetch_status_summaries(&self, context: &Context) -> Result<StatusSummaries, Error> {
        let statuses = self
            .github
            .pull_request_statuses(&context.pull_request)
            .await?;
        let mut last_run_per_status = HashMap::new();
        // Note: there's 0 docs on this so it's unclear but it seems `context` is the thing to group by.
        for status in statuses {
            last_run_per_status
                .entry(status.context.clone())
                .or_insert(status);
        }
        let mut failed_statuses = Vec::new();
        let mut pending_statuses = Vec::new();
        for (_, status) in last_run_per_status {
            let url = Self::parse_status_url(&status.target_url)?;
            let summary = StatusSummary {
                url,
                name: status.context,
            };
            match status.state {
                StatusState::Failure => failed_statuses.push(summary),
                StatusState::Pending => pending_statuses.push(summary),
                _ => (),
            };
        }
        Ok(StatusSummaries {
            pending_statuses,
            failed_statuses,
        })
    }

    fn parse_status_url(url: &str) -> Result<Url, Error> {
        let url = Url::parse(url)
            .map_err(|_| Error::as_generic(format!("invalid status target URL: {}", url)))?;
        Ok(url)
    }
}

struct StatusSummary {
    url: Url,
    name: String,
}

struct StatusSummaries {
    pending_statuses: Vec<StatusSummary>,
    failed_statuses: Vec<StatusSummary>,
}

#[async_trait]
impl Step for CheckBuildFailed {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        if !matches!(
            context.pull_request.mergeable_state,
            MergeableState::Blocked
        ) {
            return Ok(StepStatus::Passed);
        }
        if self.last_head_hash.as_ref() != Some(&context.pull_request.head.sha) {
            if self.last_head_hash.is_some() {
                info!("Resetting failure counters as the head sha changed");
                self.status_failures.clear();
            }
            self.last_head_hash = Some(context.pull_request.head.sha.clone());
        }
        let statuses_result = self.check_statuses(context).await?;
        let actions_result = self.check_actions(context).await?;
        if (statuses_result, actions_result) == (StepStatus::Passed, StepStatus::Passed)
            && context.pull_request.mergeable_state == MergeableState::Unstable
        {
            // This means we don't currently support whatever led this PR to be unstable
            return Err(Error::as_generic(
                "pull request is unstable for unknown reasons",
            ));
        } else {
            Ok(StepStatus::Waiting)
        }
    }
}

impl fmt::Display for CheckBuildFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check if CI builds failed")
    }
}
