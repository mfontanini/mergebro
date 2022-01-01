use super::steps::{
    CheckBehindMaster, CheckCurrentStateStep, CheckReviewsStep, Context, Step, StepStatus,
};
use super::Error;
use crate::github::{GithubClient, PullRequestIdentifier};
use log::{debug, info, warn};
use std::sync::Arc;

pub struct Director<G> {
    github: Arc<G>,
    identifier: PullRequestIdentifier,
    steps: Vec<Box<dyn Step>>,
}

impl<G: GithubClient + Send + Sync + 'static> Director<G> {
    pub fn new(github: G, identifier: PullRequestIdentifier) -> Self {
        let github = Arc::new(github);
        let steps = Self::build_steps(&github);
        Self {
            github,
            identifier,
            steps,
        }
    }

    fn build_steps(github: &Arc<G>) -> Vec<Box<dyn Step>> {
        vec![
            Box::new(CheckCurrentStateStep::default()),
            Box::new(CheckReviewsStep::new(github.clone())),
            Box::new(CheckBehindMaster::new(github.clone())),
        ]
    }

    pub async fn run(&mut self) -> Result<DirectorState, Error> {
        let context = self.build_context().await?;
        for step in &mut self.steps {
            let step_status = step.execute(&context).await?;
            match step_status {
                StepStatus::CannotProceed { reason } => {
                    warn!("Cannot proceed: {}", reason);
                    return Ok(DirectorState::Done);
                }
                StepStatus::Waiting => {
                    debug!("Step '{}' is pending", step.name());
                    return Ok(DirectorState::Pending);
                }
                StepStatus::Passed => debug!("Step '{}' passed", step.name()),
            };
        }
        info!("All checks passed, attempting to merge pull request");
        // TODO: attempt merging
        Ok(DirectorState::Pending)
    }

    async fn build_context(&self) -> Result<Context, Error> {
        let info = self.github.pull_request_info(&self.identifier).await?;
        Ok(Context::new(self.identifier.clone(), info))
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DirectorState {
    Done,
    Pending,
}
