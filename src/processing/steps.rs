use super::Error;
use crate::circleci::{workflows::WorkflowRunner, CircleCiClient};
use crate::github::{
    Branch, BranchProtection, GithubClient, MergeableState, PullRequest, PullRequestIdentifier,
    PullRequestReview, PullRequestState, ReviewState, StatusState, WorkflowRunConclusion,
};
use async_trait::async_trait;
use log::info;
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
    CannotProceed { reason: String },
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
                    Ok(StepStatus::CannotProceed {
                        reason: "pull request is a draft".into(),
                    })
                } else if matches!(context.pull_request.mergeable_state, MergeableState::Dirty) {
                    Ok(StepStatus::CannotProceed {
                        reason: "pull request has conflicts".into(),
                    })
                } else {
                    Ok(StepStatus::Passed)
                }
            }
            PullRequestState::Closed if context.pull_request.merged => {
                Ok(StepStatus::CannotProceed {
                    reason: "pull request is already merged".into(),
                })
            }
            PullRequestState::Closed => Ok(StepStatus::CannotProceed {
                reason: "pull request is closed".into(),
            }),
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
pub struct CheckReviewsStep<G> {
    github: Arc<G>,
}

impl<G: GithubClient> CheckReviewsStep<G> {
    pub fn new(github: Arc<G>) -> Self {
        Self { github }
    }

    async fn fetch_branch_protection(&self, branch: &Branch) -> Result<BranchProtection, Error> {
        let branch_protection = self.github.branch_protection(branch).await;
        match branch_protection {
            Ok(branch_protection) => Ok(branch_protection),
            Err(e) if e.not_found() => Ok(BranchProtection::default()),
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
}

#[async_trait]
impl<G: GithubClient + Send + Sync> Step for CheckReviewsStep<G> {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        let branch_protection = self
            .fetch_branch_protection(&context.pull_request.base)
            .await?;
        // TODO: make this minimum configurable and possibly only fall back if we get 404 on
        // the branch protection endpoint
        let approvals_needed = (branch_protection.reviews.approvals as usize).min(1);
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
            return Ok(StepStatus::CannotProceed { reason });
        } else {
            return Ok(StepStatus::Passed);
        }
    }
}

impl<G> fmt::Display for CheckReviewsStep<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check reviews")
    }
}

/// Checks whether a pull request is behind master, and updates it otherwise
pub struct CheckBehindMaster<G> {
    github: Arc<G>,
}

impl<G: GithubClient> CheckBehindMaster<G> {
    pub fn new(github: Arc<G>) -> Self {
        Self { github }
    }
}

#[async_trait]
impl<G: GithubClient + Send + Sync> Step for CheckBehindMaster<G> {
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

impl<G> fmt::Display for CheckBehindMaster<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check if behind master")
    }
}

/// Checks whether the build for a pull request failed, re-triggering CI runs if needed
pub struct CheckBuildFailed<G, C> {
    github: Arc<G>,
    circleci_runner: WorkflowRunner<C>,
}

impl<G: GithubClient, C: CircleCiClient> CheckBuildFailed<G, C> {
    pub fn new(github: Arc<G>, circleci: Arc<C>) -> Self {
        let circleci_runner = WorkflowRunner::new(circleci);
        Self {
            github,
            circleci_runner,
        }
    }

    async fn check_actions(&self, context: &Context) -> Result<StepStatus, Error> {
        // TODO: make sure head vs base is right here for forks
        let action_runs = self.github.action_runs(&context.pull_request.head).await?;
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
            info!("Actions workflow '{}' failed, re-running it", run.name);
            self.github
                .rerun_workflow(&context.pull_request.base.repo, run.id)
                .await?;
        }
        Ok(StepStatus::Waiting)
    }

    async fn check_statuses(&self, context: &Context) -> Result<StepStatus, Error> {
        let summaries = self.fetch_status_summaries(context).await?;
        match summaries.pending_statuses.len() {
            0 => {
                if summaries.failed_statuses.is_empty() {
                    return Ok(StepStatus::Passed);
                }
                info!(
                    "Processing {} failed external jobs",
                    summaries.failed_statuses.len()
                );
                let failed_job_urls = summaries.failed_statuses.iter().map(|summary| &summary.url);
                self.circleci_runner
                    .process_failed_jobs(failed_job_urls)
                    .await?;
            }
            1 => {
                info!(
                    "Waiting for external job '{}' to finish running",
                    summaries.pending_statuses[0].name
                );
            }
            _ => {
                info!(
                    "Waiting for {} external jobs to finish running",
                    summaries.pending_statuses.len()
                );
            }
        };
        Ok(StepStatus::Waiting)
    }

    async fn fetch_status_summaries(&self, context: &Context) -> Result<StatusSummaries, Error> {
        let statuses = self
            .github
            .pull_request_statuses(&context.pull_request)
            .await?;
        let mut last_run_per_status = HashMap::new();
        for status in statuses {
            let url = Self::parse_status_url(&status.target_url)?;
            last_run_per_status.entry(url).or_insert(status);
        }
        let mut failed_statuses = Vec::new();
        let mut pending_statuses = Vec::new();
        for (url, status) in last_run_per_status {
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
            .map_err(|_| Error::Generic(format!("invalid status target URL: {}", url)))?;
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
impl<G, C> Step for CheckBuildFailed<G, C>
where
    G: GithubClient + Send + Sync,
    C: CircleCiClient + Send + Sync,
{
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        // TODO: consider restricting this more
        if matches!(context.pull_request.mergeable_state, MergeableState::Clean) {
            return Ok(StepStatus::Passed);
        }
        let statuses_result = self.check_statuses(context).await?;
        let actions_result = self.check_actions(context).await?;
        if (statuses_result, actions_result) == (StepStatus::Passed, StepStatus::Passed)
            && context.pull_request.mergeable_state == MergeableState::Unstable
        {
            // This means we don't currently support whatever led this PR to be unstable
            return Err(Error::Generic(
                "pull request is unstable for unknown reasons".into(),
            ));
        } else {
            Ok(StepStatus::Waiting)
        }
    }
}

impl<G, C> fmt::Display for CheckBuildFailed<G, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check if CI builds failed")
    }
}