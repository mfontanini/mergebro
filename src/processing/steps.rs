use super::{Error, WorkflowRunner, WorkflowStatus};
use crate::{
    config::{ReviewsConfig, StatusFailuresConfig},
    github::{
        Branch, BranchProtection, GithubClient, MergeableState, PullRequest, PullRequestReview,
        PullRequestState, ReviewState, StatusState, WorkflowRunConclusion,
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
    /// Execute this step against the current state of this pull request
    async fn execute(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error>;
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
    async fn execute(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        match pull_request.state {
            PullRequestState::Open => {
                if pull_request.draft {
                    Err(Error::as_generic("pull request is a draft"))
                } else if matches!(pull_request.mergeable_state, MergeableState::Dirty) {
                    Err(Error::as_generic("pull request has conflicts"))
                } else {
                    Ok(StepStatus::Passed)
                }
            }
            PullRequestState::Closed if pull_request.merged => {
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
    reviews: ReviewsConfig,
}

impl CheckReviewsStep {
    pub fn new(
        github: Arc<dyn GithubClient>,
        reviews: ReviewsConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { github, reviews })
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

    fn required_approvals(&self, branch_protection: Option<BranchProtection>) -> u32 {
        let configured_approvals = self.reviews.approvals;
        match branch_protection {
            Some(protection) => protection.reviews.approvals.max(configured_approvals),
            None => configured_approvals,
        }
    }
}

#[async_trait]
impl Step for CheckReviewsStep {
    async fn execute(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        let branch_protection = self.fetch_branch_protection(&pull_request.base).await?;
        let approvals_needed = self.required_approvals(branch_protection) as usize;
        let reviews = self.github.pull_request_reviews(pull_request).await?;
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
    async fn execute(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        if !matches!(pull_request.mergeable_state, MergeableState::Behind) {
            return Ok(StepStatus::Passed);
        }
        let result = self.github.update_branch(pull_request).await;
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
    last_head_hash: Option<String>,
    status_failures_config: HashMap<String, StatusFailuresConfig>,
    status_failures: HashMap<String, u32>,
}

impl CheckBuildFailed {
    pub fn new(
        github: Arc<dyn GithubClient>,
        workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
        status_failures_config: HashMap<String, StatusFailuresConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            github,
            workflow_runners,
            last_head_hash: None,
            status_failures_config,
            status_failures: HashMap::default(),
        })
    }

    async fn check_actions(&self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        let action_runs = self.github.action_runs(pull_request).await?;
        let mut last_run_per_workflow = HashMap::new();
        for run in action_runs.workflow_runs {
            if run.head_sha != pull_request.head.sha {
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
                .rerun_workflow(&pull_request.base.repo, run.id)
                .await?;
        }
        Ok(StepStatus::Waiting)
    }

    async fn check_statuses(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        let summaries = self.fetch_status_summaries(pull_request).await?;
        let pending = summaries.pending_statuses;
        let pending_count = pending.len();
        match pending_count {
            0 => {
                if summaries.failed_statuses.is_empty() {
                    return Ok(StepStatus::Passed);
                }
                self.process_failed_statuses(summaries.failed_statuses)
                    .await?;
            }
            1 => {
                info!("Waiting for external job '{}' to finish", pending[0].name);
            }
            _ => {
                info!("Waiting for {} external jobs to finish", pending.len());
            }
        }
        Ok(StepStatus::Waiting)
    }

    async fn process_failed_statuses(&mut self, statuses: Vec<StatusSummary>) -> Result<(), Error> {
        warn!("Processing {} failed external jobs", statuses.len());
        self.check_max_failures(&statuses)?;
        let failed_job_urls: Vec<_> = statuses.into_iter().map(|summary| summary.url).collect();
        let mut total_triggered = 0;
        for runner in &self.workflow_runners {
            if runner.process_failed_jobs(&failed_job_urls).await? == WorkflowStatus::Triggered {
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
        Ok(())
    }

    fn check_max_failures(&mut self, failed_statuses: &[StatusSummary]) -> Result<(), Error> {
        for status in failed_statuses {
            if let Some(config) = self.status_failures_config.get(&status.name) {
                let failures = self.status_failures.entry(status.name.clone()).or_insert(0);
                *failures += 1;
                if *failures >= config.max_failures {
                    return Err(Error::as_generic(format!(
                        "status check '{}' reached {} failures",
                        status.name, failures
                    )));
                }
            }
        }
        Ok(())
    }

    async fn fetch_status_summaries(
        &self,
        pull_request: &PullRequest,
    ) -> Result<StatusSummaries, Error> {
        let statuses = self.github.pull_request_statuses(pull_request).await?;
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
    async fn execute(&mut self, pull_request: &PullRequest) -> Result<StepStatus, Error> {
        if !matches!(pull_request.mergeable_state, MergeableState::Blocked) {
            return Ok(StepStatus::Passed);
        }
        if self.last_head_hash.as_ref() != Some(&pull_request.head.sha) {
            if self.last_head_hash.is_some() {
                info!("Resetting failure counters as the head sha changed");
                self.status_failures.clear();
            }
            self.last_head_hash = Some(pull_request.head.sha.clone());
        }
        let statuses_result = self.check_statuses(pull_request).await?;
        let actions_result = self.check_actions(pull_request).await?;
        if (statuses_result, actions_result) == (StepStatus::Passed, StepStatus::Passed)
            && pull_request.mergeable_state == MergeableState::Blocked
        {
            // This means we don't currently support/know whatever led this PR to be unstable
            return Err(Error::as_generic(
                "pull request is blocked for unknown reasons",
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
