use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Client(#[from] crate::client::Error),

    #[error("unsupported pull request state: {0}")]
    UnsupportedPullRequestState(String),

    #[error("{0}")]
    Generic(String),
}
