use regex::Regex;
use serde_derive::Deserialize;
use thiserror::Error;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum MergeStateStatus {
    #[serde(rename = "behind")]
    Behind,

    #[serde(rename = "clean")]
    Clean,

    #[serde(rename = "blocked")]
    Blocked,

    #[serde(rename = "blocked")]
    Unstable,

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

impl AsRef<str> for Link {
    fn as_ref(&self) -> &str {
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
    #[serde(rename = "mergeable_state")]
    pub merge_status: MergeStateStatus,

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
    pub fn from_app_url(url: &str) -> Result<Self, UrlParseError> {
        lazy_static! {
            static ref RE: Regex =
                Regex::new(r"^https://github.com/([\w_-]+)/([\w_-]+)/pull/([\d]+)$").unwrap();
        }
        if let Some(capture) = RE.captures_iter(url).next() {
            let pull_request_url = Self {
                owner: capture[1].into(),
                repo: capture[2].into(),
                pull_number: capture[3].parse().unwrap(),
            };
            Ok(pull_request_url)
        } else {
            Err(UrlParseError::MalformedUrl)
        }
    }
}

#[derive(Error, Debug, PartialEq, Clone)]
pub enum UrlParseError {
    #[error("malformed URL")]
    MalformedUrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_from_app_url() {
        let pr = PullRequestIdentifier::from_app_url("https://github.com/potato/smasher/pull/1337")
            .unwrap();
        assert_eq!(pr.owner, "potato");
        assert_eq!(pr.repo, "smasher");
        assert_eq!(pr.pull_number, 1337);

        assert!(
            PullRequestIdentifier::from_app_url("https://github.com/potato/smasher/pull/").is_err()
        );
        assert!(PullRequestIdentifier::from_app_url("https://github.com//smasher/pull/").is_err());
        assert!(
            PullRequestIdentifier::from_app_url("https://github.com/potato/pull/1337").is_err()
        );
    }
}
