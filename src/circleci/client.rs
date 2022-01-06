use super::{Job, NoBody};
use crate::client::{ApiClient, Result};
use async_trait::async_trait;
use serde_derive::Serialize;

#[async_trait]
pub trait CircleCiClient {
    async fn job_info(&self, owner: &str, repo: &str, job_id: u64) -> Result<Job>;
    async fn rerun_workflow(&self, workflow_id: &str) -> Result<NoBody>;
}

pub struct DefaultCircleCiClient {
    client: ApiClient,
}

impl DefaultCircleCiClient {
    const API_BASE: &'static str = "https://circleci.com/api/v2";

    pub fn new<U: Into<String>>(username: U) -> Self {
        Self {
            client: ApiClient::from_username(username),
        }
    }
}

#[async_trait]
impl CircleCiClient for DefaultCircleCiClient {
    async fn job_info(&self, owner: &str, repo: &str, job_id: u64) -> Result<Job> {
        let url = format!(
            "{}/project/gh/{}/{}/job/{}",
            Self::API_BASE,
            owner,
            repo,
            job_id
        );
        self.client.get(&url).await
    }

    async fn rerun_workflow(&self, workflow_id: &str) -> Result<NoBody> {
        let url = format!("{}/workflow/{}/rerun", Self::API_BASE, workflow_id);
        let body = RerunWorkflowBody { from_failed: true };
        self.client.post(&url, &body).await
    }
}

#[derive(Serialize, Debug)]
struct RerunWorkflowBody {
    from_failed: bool,
}
