use crate::github::{
    client::{GithubClient, MergeRequestBody},
    MergeMethod, PullRequest, PullRequestIdentifier,
};
use crate::processing::Error;
use async_trait::async_trait;
use log::{info, warn};
use serde_derive::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct MergeConfig {
    pub default_merge_method: MergeMethod,
}

#[async_trait]
pub trait PullRequestMerger {
    async fn merge(
        &self,
        id: &PullRequestIdentifier,
        pull_request: &PullRequest,
        github: &dyn GithubClient,
    ) -> Result<(), Error>;
}

pub struct DefaultPullRequestMerger {
    config: MergeConfig,
}

impl DefaultPullRequestMerger {
    pub fn new(config: MergeConfig) -> Self {
        Self { config }
    }

    async fn merge_with_method(
        &self,
        id: &PullRequestIdentifier,
        pull_request: &PullRequest,
        github: &dyn GithubClient,
        method: &MergeMethod,
    ) -> Result<(), crate::client::Error> {
        let commit_message = Self::build_merge_message(pull_request, method);
        let request_body = MergeRequestBody {
            sha: pull_request.head.sha.clone(),
            commit_title: pull_request.title.clone(),
            commit_message,
            merge_method: method.clone(),
        };
        github.merge_pull_request(id, &request_body).await?;
        Ok(())
    }

    fn build_merge_message(pull_request: &PullRequest, method: &MergeMethod) -> Option<String> {
        if matches!(method, MergeMethod::Squash) {
            pull_request.body.clone()
        } else {
            None
        }
    }

    fn build_methods(&self) -> Vec<MergeMethod> {
        let all_methods = [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];
        let mut methods = vec![self.config.default_merge_method.clone()];
        methods.extend(
            all_methods
                .into_iter()
                .filter(|method| *method != self.config.default_merge_method),
        );
        methods
    }
}

#[async_trait]
impl PullRequestMerger for DefaultPullRequestMerger {
    async fn merge(
        &self,
        id: &PullRequestIdentifier,
        pull_request: &PullRequest,
        github: &dyn GithubClient,
    ) -> Result<(), Error> {
        let methods = self.build_methods();
        for method in methods {
            info!(
                "Attempting to merge pull request using '{:?}' merge method",
                method
            );
            match self
                .merge_with_method(id, pull_request, github, &method)
                .await
            {
                Ok(_) => {
                    info!("Pull request merged ✔️");
                    return Ok(());
                }
                Err(e) if e.method_not_allowed() => {
                    warn!("Merge method '{:?}' not allowed", method);
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Generic("No merge method allowed".into()))
    }
}

#[derive(Default)]
pub struct DummyPullRequestMerger;

#[async_trait]
impl PullRequestMerger for DummyPullRequestMerger {
    async fn merge(
        &self,
        _id: &PullRequestIdentifier,
        _pull_request: &PullRequest,
        _github: &dyn GithubClient,
    ) -> Result<(), crate::processing::Error> {
        info!("Skipping pull request merge step");
        Ok(())
    }
}
