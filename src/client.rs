use backoff::{backoff::Backoff, ExponentialBackoff};
use log::info;
use reqwest::{Client, ClientBuilder, RequestBuilder, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::future::Future;
use thiserror::Error;
use tokio::time::sleep;

static USER_AGENT: &str = "mergebro";

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    username: String,
    password: Option<String>,
}

impl ApiClient {
    pub fn from_username<S: Into<String>>(username: S) -> Self {
        ApiClient::new(username.into(), None)
    }

    pub fn from_credentials<U: Into<String>, P: Into<String>>(username: U, password: P) -> Self {
        ApiClient::new(username.into(), Some(password.into()))
    }

    fn new(username: String, password: Option<String>) -> Self {
        let client = ClientBuilder::new().user_agent(USER_AGENT).build().unwrap();
        Self {
            client,
            username,
            password,
        }
    }

    pub async fn get<O>(&self, endpoint: &str) -> Result<O>
    where
        O: DeserializeOwned + Debug,
    {
        retry_request_if_needed(|| {
            let builder = self.client.get(endpoint);
            self.submit(builder)
        })
        .await
    }

    pub async fn post<I, O>(&self, endpoint: &str, body: &I) -> Result<O>
    where
        I: Serialize,
        O: DeserializeOwned + Debug,
    {
        retry_request_if_needed(|| {
            let builder = self.client.post(endpoint).json(body);
            self.submit(builder)
        })
        .await
    }

    pub async fn put<I, O>(&self, endpoint: &str, body: &I) -> Result<O>
    where
        I: Serialize,
        O: DeserializeOwned + Debug,
    {
        retry_request_if_needed(|| {
            let builder = self.client.put(endpoint).json(body);
            self.submit(builder)
        })
        .await
    }

    async fn submit<O>(&self, builder: RequestBuilder) -> Result<O>
    where
        O: DeserializeOwned,
    {
        let builder = builder.basic_auth(&self.username, self.password.as_ref());
        let response = builder.send().await?;
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(Error::Http(response.status()))
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("rate limited max attempts reached")]
    RateLimitRetries,

    #[error("request failed with status code {0}")]
    Http(StatusCode),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

impl Error {
    pub fn not_found(&self) -> bool {
        matches!(self, Self::Http(StatusCode::NOT_FOUND))
    }

    pub fn unprocessable_entity(&self) -> bool {
        matches!(self, Self::Http(StatusCode::UNPROCESSABLE_ENTITY))
    }

    pub fn method_not_allowed(&self) -> bool {
        matches!(self, Self::Http(StatusCode::METHOD_NOT_ALLOWED))
    }

    pub fn too_many_requests(&self) -> bool {
        matches!(self, Self::Http(StatusCode::TOO_MANY_REQUESTS))
    }

    pub fn conflict(&self) -> bool {
        matches!(self, Self::Http(StatusCode::CONFLICT))
    }
}

async fn retry_request_if_needed<F, R, O>(requestor: F) -> Result<O>
where
    F: Fn() -> R,
    R: Future<Output = Result<O>>,
    O: DeserializeOwned + Debug,
{
    // TODO: make configurable
    let mut backoff = ExponentialBackoff::default();
    loop {
        match requestor().await {
            Err(e) if e.too_many_requests() => {
                let delay = backoff.next_backoff();
                match delay {
                    Some(delay) => {
                        info!("Rate limit hit, sleeping for {}s", delay.as_secs());
                        sleep(delay).await
                    }
                    None => return Err(Error::RateLimitRetries),
                }
            }
            other => return other,
        }
    }
}
