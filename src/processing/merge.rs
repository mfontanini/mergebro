use crate::config::MergeConfig;
use crate::github::{
    client::{GithubClient, MergeRequestBody},
    MergeMethod, PullRequest, PullRequestIdentifier,
};
use crate::processing::Error;
use async_trait::async_trait;
use log::{info, warn};

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
    merge_methods: Vec<MergeMethod>,
}

impl DefaultPullRequestMerger {
    pub fn new(config: MergeConfig) -> Self {
        let merge_methods = Self::build_merge_methods(config.default_method);
        Self { merge_methods }
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

    fn build_merge_methods(default_method: MergeMethod) -> Vec<MergeMethod> {
        let mut methods = vec![MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];
        let default_index = methods
            .iter()
            .position(|element| element == &default_method)
            .unwrap();
        methods.swap(default_index, 0);
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
        for method in &self.merge_methods {
            info!(
                "Attempting to merge pull request using '{:?}' merge method",
                method
            );
            match self
                .merge_with_method(id, pull_request, github, method)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn test_build_merge_methods(
        #[values(MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase)] method: MergeMethod,
    ) {
        let methods = DefaultPullRequestMerger::build_merge_methods(method.clone());
        assert_eq!(methods.len(), 3);
        assert_eq!(methods[0], method);
        for method in [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase] {
            assert!(methods.iter().position(|m| m == &method).is_some());
        }
    }
}
