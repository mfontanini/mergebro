use super::Error;
use async_trait::async_trait;
use reqwest::Url;

#[async_trait]
pub trait WorkflowRunner: Send + Sync {
    async fn process_failed_jobs(&self, job_urls: &[Url]) -> Result<(), Error>;
}
