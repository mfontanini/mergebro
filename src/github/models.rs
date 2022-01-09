use reqwest::Url;
use serde_derive::{Deserialize, Serialize};
use std::ops::Deref;
use thiserror::Error;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum MergeableState {
    #[serde(rename = "behind")]
    Behind,

    #[serde(rename = "clean")]
    Clean,

    #[serde(rename = "blocked")]
    Blocked,

    #[serde(rename = "unstable")]
    Unstable,

    #[serde(rename = "dirty")]
    Dirty,

    #[serde(other, rename = "unknown")]
    Unknown,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum PullRequestState {
    #[serde(rename = "open")]
    Open,

    #[serde(rename = "closed")]
    Closed,

    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Link {
    href: String,
}

impl Deref for Link {
    type Target = str;

    fn deref(&self) -> &str {
        &self.href
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Links {
    pub statuses: Link,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct User {
    pub login: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum ReviewState {
    #[serde(rename = "APPROVED")]
    Approved,

    #[serde(rename = "CHANGES_REQUESTED")]
    ChangesRequested,

    #[serde(rename = "COMMENTED")]
    Commented,

    #[serde(rename = "DISMISSED")]
    Dismissed,

    #[serde(rename = "PENDING")]
    Pending,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct PullRequestReview {
    pub user: User,
    pub state: ReviewState,
    pub submitted_at: chrono::DateTime<chrono::Local>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Repository {
    pub name: String,
    pub owner: User,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Branch {
    pub sha: String,

    #[serde(rename = "ref")]
    pub name: String,

    pub user: User,
    pub repo: Repository,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct PullRequest {
    pub mergeable_state: MergeableState,

    #[serde(rename = "_links")]
    pub links: Links,

    #[serde(rename = "user")]
    pub creator: User,

    pub state: PullRequestState,
    pub title: String,
    pub head: Branch,
    pub base: Branch,
    pub merged: bool,
    pub draft: bool,
    pub body: Option<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
pub struct BranchProtection {
    #[serde(rename = "required_pull_request_reviews")]
    pub reviews: BranchProtectionReviews,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
pub struct BranchProtectionReviews {
    #[serde(rename = "required_approving_review_count")]
    pub approvals: u32,
}

#[derive(Debug, PartialEq, Clone)]
pub struct PullRequestIdentifier {
    pub owner: String,
    pub repo: String,
    pub pull_number: u32,
}

impl PullRequestIdentifier {
    pub fn from_app_url(url: &Url) -> Result<Self, InvalidUrlError> {
        if url.domain() != Some("github.com") {
            return Err(InvalidUrlError::InvalidDomain);
        }
        let path_parts: Vec<_> = url
            .path_segments()
            .ok_or(InvalidUrlError::NotPullRequestUrl)?
            .collect();
        if path_parts.len() != 4 || path_parts[2] != "pull" {
            return Err(InvalidUrlError::NotPullRequestUrl);
        }
        let pull_number = path_parts[3]
            .parse()
            .map_err(|_| InvalidUrlError::NotPullRequestUrl)?;
        let pull_request_url = Self {
            owner: path_parts[0].into(),
            repo: path_parts[1].into(),
            pull_number,
        };
        Ok(pull_request_url)
    }
}

#[derive(Error, Debug, PartialEq, Clone)]
pub enum InvalidUrlError {
    #[error("invalid domain")]
    InvalidDomain,

    #[error("not a pull request URL")]
    NotPullRequestUrl,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ActionRuns {
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct WorkflowRun {
    pub id: u64,
    pub workflow_id: u64,
    pub name: String,
    pub head_sha: String,
    pub status: WorfklowRunStatus,
    pub conclusion: Option<WorkflowRunConclusion>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum WorfklowRunStatus {
    #[serde(rename = "completed")]
    Completed,

    #[serde(rename = "queued")]
    Queued,

    #[serde(rename = "in_progress")]
    InProgress,

    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum WorkflowRunConclusion {
    #[serde(rename = "success")]
    Success,

    #[serde(rename = "failure")]
    Failure,

    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub target_url: String,
    pub state: StatusState,
    pub created_at: chrono::DateTime<chrono::Local>,
    pub context: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum StatusState {
    #[serde(rename = "success")]
    Success,

    #[serde(rename = "failure")]
    Failure,

    #[serde(rename = "pending")]
    Pending,

    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum MergeMethod {
    #[serde(rename = "merge")]
    Merge,

    #[serde(rename = "squash")]
    Squash,

    #[serde(rename = "rebase")]
    Rebase,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct NoBody {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_from_app_url() {
        let pr = PullRequestIdentifier::from_app_url(
            &Url::parse("https://github.com/potato/smasher/pull/1337").unwrap(),
        )
        .unwrap();
        assert_eq!(pr.owner, "potato");
        assert_eq!(pr.repo, "smasher");
        assert_eq!(pr.pull_number, 1337);

        assert!(PullRequestIdentifier::from_app_url(
            &Url::parse("https://github.com/potato/smasher/pull/").unwrap()
        )
        .is_err());
        assert!(PullRequestIdentifier::from_app_url(
            &Url::parse("https://github.com//smasher/pull/").unwrap()
        )
        .is_err());
        assert!(PullRequestIdentifier::from_app_url(
            &Url::parse("https://github.com/potato/pull/1337").unwrap()
        )
        .is_err());
    }
}
