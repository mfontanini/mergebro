use super::models::{
    Branch, BranchProtection, PullRequest, PullRequestIdentifier, PullRequestReview,
};
use crate::client::{ApiClient, Result};
use async_trait::async_trait;

#[async_trait]
pub trait GithubClient {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest>;

    async fn pull_request_reviews(
        &self,
        id: &PullRequestIdentifier,
    ) -> Result<Vec<PullRequestReview>>;

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection>;
}

#[derive(Clone)]
pub struct DefaultGithubClient {
    client: ApiClient,
}

impl DefaultGithubClient {
    pub fn new<U: Into<String>, P: Into<String>>(username: U, password: P) -> Self {
        Self {
            client: ApiClient::from_credentials(username, password),
        }
    }
}

#[async_trait]
impl GithubClient for DefaultGithubClient {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls/{}",
            id.owner, id.repo, id.pull_number
        );
        self.client.get(&url).await
    }

    async fn pull_request_reviews(
        &self,
        id: &PullRequestIdentifier,
    ) -> Result<Vec<PullRequestReview>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls/{}/reviews",
            id.owner, id.repo, id.pull_number
        );
        self.client.get(&url).await
    }

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/branches/{}/protection",
            branch.user.login, branch.repo.name, branch.name,
        );
        self.client.get(&url).await
    }
}
