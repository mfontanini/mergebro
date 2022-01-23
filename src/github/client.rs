use super::models::{
    ActionRuns, Branch, BranchProtection, NoBody, PullRequest, PullRequestIdentifier,
    PullRequestReview, Repository, Status,
};
use crate::client::{ApiClient, Result};
use crate::github::MergeMethod;
use async_trait::async_trait;
use serde_derive::Serialize;

#[async_trait]
pub trait GithubClient: Send + Sync {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest>;
    async fn pull_request_reviews(
        &self,
        pull_request: &PullRequest,
    ) -> Result<Vec<PullRequestReview>>;
    async fn pull_request_statuses(&self, pull_request: &PullRequest) -> Result<Vec<Status>>;
    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection>;
    async fn update_branch(&self, pull_request: &PullRequest) -> Result<NoBody>;
    async fn action_runs(&self, pull_request: &PullRequest) -> Result<ActionRuns>;
    async fn rerun_workflow(&self, repo: &Repository, run_id: u64) -> Result<NoBody>;
    async fn merge_pull_request(
        &self,
        pull_request: &PullRequest,
        body: &MergeRequestBody,
    ) -> Result<NoBody>; // TODO: add body
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeRequestBody {
    pub sha: String,
    pub commit_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
    pub merge_method: MergeMethod,
}

#[derive(Clone)]
pub struct DefaultGithubClient {
    client: ApiClient,
}

impl DefaultGithubClient {
    const API_BASE: &'static str = "https://api.github.com";

    pub fn new<U: Into<String>, P: Into<String>>(username: U, password: P) -> Self {
        Self {
            client: ApiClient::from_credentials(username, password),
        }
    }

    fn make_pull_request_url(pull_request: &PullRequest) -> String {
        format!(
            "{}/repos/{}/pulls/{}",
            Self::API_BASE,
            pull_request.base.repo.full_name,
            pull_request.number
        )
    }
}

#[async_trait]
impl GithubClient for DefaultGithubClient {
    async fn pull_request_info(&self, id: &PullRequestIdentifier) -> Result<PullRequest> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            Self::API_BASE,
            id.owner,
            id.repo,
            id.pull_number
        );
        self.client.get(&url).await
    }

    async fn pull_request_reviews(
        &self,
        pull_request: &PullRequest,
    ) -> Result<Vec<PullRequestReview>> {
        let url = format!("{}/reviews", Self::make_pull_request_url(pull_request));
        self.client.get(&url).await
    }

    async fn pull_request_statuses(&self, pull_request: &PullRequest) -> Result<Vec<Status>> {
        self.client.get(&pull_request.links.statuses).await
    }

    async fn branch_protection(&self, branch: &Branch) -> Result<BranchProtection> {
        let url = format!(
            "{}/repos/{}/{}/branches/{}/protection",
            Self::API_BASE,
            branch.user.login,
            branch.repo.name,
            branch.name,
        );
        self.client.get(&url).await
    }

    async fn update_branch(&self, pull_request: &PullRequest) -> Result<NoBody> {
        let url = format!(
            "{}/update-branch",
            Self::make_pull_request_url(pull_request)
        );
        let body = UpdateBranchRequest {
            expected_head_sha: pull_request.head.sha.clone(),
        };
        self.client.put(&url, &body).await
    }

    async fn action_runs(&self, pull_request: &PullRequest) -> Result<ActionRuns> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs?branch={}&actor={}",
            Self::API_BASE,
            pull_request.base.repo.owner.login,
            pull_request.base.repo.name,
            pull_request.head.name,
            pull_request.head.repo.owner.login
        );
        self.client.get(&url).await
    }

    async fn rerun_workflow(&self, repo: &Repository, run_id: u64) -> Result<NoBody> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/rerun",
            Self::API_BASE,
            repo.owner.login,
            repo.name,
            run_id,
        );
        self.client.post(&url, &()).await
    }

    async fn merge_pull_request(
        &self,
        pull_request: &PullRequest,
        body: &MergeRequestBody,
    ) -> Result<NoBody> {
        let url = format!("{}/merge", Self::make_pull_request_url(pull_request));
        self.client.put(&url, body).await
    }
}

#[derive(Serialize, Debug, PartialEq)]
struct UpdateBranchRequest {
    expected_head_sha: String,
}
