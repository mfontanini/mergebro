use super::steps::{Context, Step, StepStatus};
use super::{Error, PullRequestMerger};
use crate::github::{GithubClient, PullRequestIdentifier};
use log::{debug, info};
use std::sync::Arc;

pub struct Director {
    github: Arc<dyn GithubClient>,
    identifier: PullRequestIdentifier,
    steps: Vec<Box<dyn Step>>,
    merger: Arc<dyn PullRequestMerger>,
}

impl Director {
    pub fn new(
        github: Arc<dyn GithubClient>,
        merger: Arc<dyn PullRequestMerger>,
        steps: Vec<Box<dyn Step>>,
        identifier: PullRequestIdentifier,
    ) -> Self {
        Self {
            github,
            identifier,
            steps,
            merger,
        }
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
        info!("All checks passed, pull request is ready to be merged!");
        self.merger
            .merge(&context.identifier, &context.pull_request, &*self.github)
            .await?;
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
