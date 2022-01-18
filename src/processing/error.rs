use std::borrow::Cow;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Client(#[from] crate::client::Error),

    #[error("unsupported pull request state: {0}")]
    UnsupportedPullRequestState(Cow<'static, str>),

    #[error("{0}")]
    Generic(Cow<'static, str>),
}

impl Error {
    pub fn as_generic<T>(message: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        Self::Generic(message.into())
    }
}
