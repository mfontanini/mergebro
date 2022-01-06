use super::steps::{
    CheckBehindMaster, CheckBuildFailed, CheckCurrentStateStep, CheckReviewsStep, Context, Step,
    StepStatus,
};
use super::Error;
use crate::circleci::CircleCiClient;
use crate::github::{GithubClient, PullRequestIdentifier};
use log::{debug, info, warn};
use std::sync::Arc;

pub struct Director<G> {
    github: Arc<G>,
    identifier: PullRequestIdentifier,
    steps: Vec<Box<dyn Step>>,
}

impl<G> Director<G>
where
    G: GithubClient + Send + Sync + 'static,
{
    pub fn new<C>(github: G, circleci: C, identifier: PullRequestIdentifier) -> Self
    where
        C: CircleCiClient + Send + Sync + 'static,
    {
        let github = Arc::new(github);
        let circleci = Arc::new(circleci);
        let steps = Self::build_steps(&github, &circleci);
        Self {
            github,
            identifier,
            steps,
        }
    }

    fn build_steps<C>(github: &Arc<G>, circleci: &Arc<C>) -> Vec<Box<dyn Step>>
    where
        C: CircleCiClient + Send + Sync + 'static,
    {
        vec![
            Box::new(CheckCurrentStateStep::default()),
            Box::new(CheckReviewsStep::new(github.clone())),
            Box::new(CheckBehindMaster::new(github.clone())),
            Box::new(CheckBuildFailed::new(github.clone(), circleci.clone())),
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
                    debug!("Step '{}' is pending", step);
                    return Ok(DirectorState::Pending);
                }
                StepStatus::Passed => debug!("Step '{}' passed", step),
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
