use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("client: {0}")]
    Client(#[from] crate::client::Error),
}
