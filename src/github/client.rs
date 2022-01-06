use super::models::{
    ActionRuns, Branch, BranchProtection, NoBody, PullRequest, PullRequestIdentifier,
    PullRequestReview, Repository, Status,
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
    async fn pull_request_statuses(&self, pull_request: &PullRequest) -> Result<Vec<Status>>;
    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection>;
    async fn update_branch(&self, id: &PullRequestIdentifier, head_sha: &str) -> Result<NoBody>;
    async fn action_runs(&self, branch: &Branch) -> Result<ActionRuns>;
    async fn rerun_workflow(&self, repo: &Repository, run_id: u64) -> Result<NoBody>;
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

    async fn pull_request_statuses(&self, pull_request: &PullRequest) -> Result<Vec<Status>> {
        self.client.get(&pull_request.links.statuses).await
    }

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/branches/{}/protection",
            branch.user.login, branch.repo.name, branch.name,
        );
        self.client.get(&url).await
    }

    async fn update_branch(&self, id: &PullRequestIdentifier, head_sha: &str) -> Result<NoBody> {
        let url = format!("{}/update-branch", Self::make_pull_request_url(id));
        let body = UpdateBranchRequest {
            expected_head_sha: head_sha.into(),
        };
        self.client.put(&url, &body).await
    }

    async fn action_runs(&self, branch: &Branch) -> Result<ActionRuns> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs?branch={}",
            branch.repo.owner.login, branch.repo.name, branch.name,
        );
        self.client.get(&url).await
    }

    async fn rerun_workflow(&self, repo: &Repository, run_id: u64) -> Result<NoBody> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs/{}/rerun",
            repo.owner.login, repo.name, run_id,
        );
        self.client.post(&url, &()).await
    }
}

#[derive(Serialize, Debug, PartialEq)]
struct UpdateBranchRequest {
    expected_head_sha: String,
}
