use super::Error;
use crate::github::{
    Branch, BranchProtection, GithubClient, MergeableState, PullRequest, PullRequestIdentifier,
    PullRequestReview, PullRequestState, ReviewState, WorkflowRunConclusion,
};
use async_trait::async_trait;
use log::info;
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
    identifier: PullRequestIdentifier,
    pull_request: PullRequest,
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
            PullRequestState::Open if context.pull_request.draft => Ok(StepStatus::CannotProceed {
                reason: "pull request is a draft".into(),
            }),
            PullRequestState::Open => Ok(StepStatus::Passed),
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
        let approvals_needed = branch_protection.reviews.approvals as usize;
        if approvals_needed == 0 {
            return Ok(StepStatus::Passed);
        }
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
pub struct CheckBuildFailed<G> {
    github: Arc<G>,
}

impl<G: GithubClient> CheckBuildFailed<G> {
    pub fn new(github: Arc<G>) -> Self {
        Self { github }
    }

    async fn check_actions(&self, context: &Context) -> Result<(), Error> {
        // TODO: make sure head vs base is right here for forks
        let action_runs = self.github.action_runs(&context.pull_request.head).await?;
        let mut last_run_per_workflow = HashMap::new();
        for run in action_runs.workflow_runs {
            if run.head_sha != context.pull_request.head.sha {
                continue;
            }
            last_run_per_workflow.entry(run.workflow_id).or_insert(run);
        }
        if last_run_per_workflow.is_empty() {
            // This means we currently can't handle whatever led the PR to be unstable
            return Err(Error::UnsupportedPullRequestState(
                "reason why pull request is unstable is unknown".into(),
            ));
        }
        for run in last_run_per_workflow.values() {
            if run.conclusion == Some(WorkflowRunConclusion::Failure) {
                info!("Workflow '{}' failed, re-running it", run.name);
                self.github
                    .rerun_workflow(&context.pull_request.base.repo, run.id)
                    .await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<G: GithubClient + Send + Sync> Step for CheckBuildFailed<G> {
    async fn execute(&mut self, context: &Context) -> Result<StepStatus, Error> {
        if !matches!(
            context.pull_request.mergeable_state,
            MergeableState::Unstable
        ) {
            return Ok(StepStatus::Passed);
        }
        // TODO: check for status checks as well
        self.check_actions(context).await?;
        Ok(StepStatus::Waiting)
    }
}

impl<G> fmt::Display for CheckBuildFailed<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check if build failed")
    }
}
