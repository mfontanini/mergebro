use super::steps::{Step, StepStatus};
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
        debug!("Fetching current state for pull request");
        let pull_request = self.github.pull_request_info(&self.identifier).await?;
        for step in &mut self.steps {
            let step_status = step.execute(&pull_request).await?;
            match step_status {
                StepStatus::Waiting => {
                    info!("Step '{}' is pending", step);
                    return Ok(DirectorState::Waiting);
                }
                StepStatus::Passed => debug!("Step '{}' passed", step),
            };
        }
        info!("All checks passed, pull request is ready to be merged!");
        self.merger.merge(&pull_request, &*self.github).await?;
        Ok(DirectorState::Done)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DirectorState {
    Done,
    Waiting,
}
