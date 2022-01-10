use super::steps::{
    CheckBehindMaster, CheckBuildFailed, CheckCurrentStateStep, CheckReviewsStep, Context, Step,
    StepStatus,
};
use super::{Error, PullRequestMerger, WorkflowRunner};
use crate::github::{GithubClient, PullRequestIdentifier};
use log::{debug, info};
use std::sync::Arc;

pub struct Director<G> {
    github: Arc<G>,
    identifier: PullRequestIdentifier,
    steps: Vec<Box<dyn Step>>,
    merger: PullRequestMerger,
}

impl<G> Director<G>
where
    G: GithubClient + Send + Sync + 'static,
{
    pub fn new(
        github: G,
        workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
        identifier: PullRequestIdentifier,
        merger: PullRequestMerger,
    ) -> Self {
        let github = Arc::new(github);
        let steps = Self::build_steps(&github, workflow_runners);
        Self {
            github,
            identifier,
            steps,
            merger,
        }
    }

    fn build_steps(
        github: &Arc<G>,
        workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
    ) -> Vec<Box<dyn Step>> {
        vec![
            Box::new(CheckCurrentStateStep::default()),
            Box::new(CheckReviewsStep::new(github.clone())),
            Box::new(CheckBehindMaster::new(github.clone())),
            Box::new(CheckBuildFailed::new(github.clone(), workflow_runners)),
        ]
    }

    pub async fn run(&mut self) -> Result<DirectorState, Error> {
        let context = self.build_context().await?;
        for step in &mut self.steps {
            let step_status = step.execute(&context).await?;
            match step_status {
                StepStatus::Waiting => {
                    info!("Step '{}' is pending", step);
                    return Ok(DirectorState::Waiting);
                }
                StepStatus::Passed => debug!("Step '{}' passed", step),
            };
        }
        info!("All checks passed, attempting to merge pull request");
        self.merger
            .merge(&context.identifier, &context.pull_request, &*self.github)
            .await?;
        info!("Pull request merged ✔️");
        Ok(DirectorState::Done)
    }

    async fn build_context(&self) -> Result<Context, Error> {
        debug!("Fetching pull request context");
        let info = self.github.pull_request_info(&self.identifier).await?;
        Ok(Context::new(self.identifier.clone(), info))
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DirectorState {
    Done,
    Waiting,
}
