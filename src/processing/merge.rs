use crate::github::{
    client::{GithubClient, MergeRequestBody},
    MergeMethod, PullRequest, PullRequestIdentifier,
};
use crate::processing::Error;
use log::info;
use serde_derive::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct MergeConfig {
    pub default_merge_method: MergeMethod,
}

pub struct PullRequestMerger {
    config: MergeConfig,
}

impl PullRequestMerger {
    pub fn new(config: MergeConfig) -> Self {
        Self { config }
    }

    pub async fn merge<G: GithubClient>(
        &self,
        id: &PullRequestIdentifier,
        pull_request: &PullRequest,
        github: &G,
    ) -> Result<(), Error> {
        let methods = self.build_methods();
        for method in methods {
            info!("Attempting to merge using '{:?}' merge method", method);
            match self
                .merge_with_method(id, pull_request, github, method)
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) if e.method_not_allowed() => {
                    info!("Merge method not allowed");
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Generic("No merge method allowed".into()))
    }

    async fn merge_with_method<G: GithubClient>(
        &self,
        id: &PullRequestIdentifier,
        pull_request: &PullRequest,
        github: &G,
        method: MergeMethod,
    ) -> Result<(), crate::client::Error> {
        let commit_message = Self::build_merge_message(pull_request, &method);
        let request_body = MergeRequestBody {
            sha: pull_request.head.sha.clone(),
            commit_title: pull_request.title.clone(),
            commit_message,
            merge_method: method,
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
