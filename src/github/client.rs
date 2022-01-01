use super::models::{
    Branch, BranchProtection, PullRequest, PullRequestIdentifier, PullRequestReview,
};
use crate::client::{ApiClient, Result};
use async_trait::async_trait;
use serde_derive::Serialize;

#[async_trait]
pub trait GithubClient {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest>;

    async fn pull_request_reviews(
        &self,
        id: &PullRequestIdentifier,
    ) -> Result<Vec<PullRequestReview>>;

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection>;

    async fn update_branch(&self, id: &PullRequestIdentifier, head_sha: &str) -> Result<()>;
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

    fn make_pull_request_url(id: &PullRequestIdentifier) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/pulls/{}",
            id.owner, id.repo, id.pull_number
        )
    }
}

#[async_trait]
impl GithubClient for DefaultGithubClient {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest> {
        let url = Self::make_pull_request_url(id);
        self.client.get(&url).await
    }

    async fn pull_request_reviews(
        &self,
        id: &PullRequestIdentifier,
    ) -> Result<Vec<PullRequestReview>> {
        let url = format!("{}/reviews", Self::make_pull_request_url(id));
        self.client.get(&url).await
    }

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/branches/{}/protection",
            branch.user.login, branch.repo.name, branch.name,
        );
        self.client.get(&url).await
    }

    async fn update_branch(&self, id: &PullRequestIdentifier, head_sha: &str) -> Result<()> {
        let url = format!("{}/update-branch", Self::make_pull_request_url(id));
        let body = UpdateBranchRequest {
            expected_head_sha: head_sha.into(),
        };
        self.client.put(&url, &body).await
    }
}

#[derive(Serialize, Debug, PartialEq)]
struct UpdateBranchRequest {
    expected_head_sha: String,
}
