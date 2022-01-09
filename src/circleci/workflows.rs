use std::collections::HashSet;

use super::CircleCiClient;
use crate::processing::Error;
use log::info;
use reqwest::Url;
use std::sync::Arc;

pub struct WorkflowRunner<C> {
    client: Arc<C>,
}

impl<C: CircleCiClient> WorkflowRunner<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn process_failed_jobs(
        &self,
        job_urls: impl Iterator<Item = &Url>,
    ) -> Result<(), Error> {
        let mut failed_workflow_ids = HashSet::new();
        for job_url in job_urls {
            let (owner, repo, job_id) = match Self::parse_job_url(job_url)? {
                JobUrl::Job {
                    owner,
                    repo,
                    job_id,
                } => (owner, repo, job_id),
                JobUrl::Unrelated => continue,
            };
            let job_info = self.client.job_info(owner, repo, job_id).await?;
            failed_workflow_ids.insert(job_info.latest_workflow.id);
        }
        if failed_workflow_ids.is_empty() {
            return Ok(());
        }
        info!(
            "Re-running {} failed circleci workflows",
            failed_workflow_ids.len()
        );
        for workflow_id in failed_workflow_ids {
            self.client.rerun_workflow(&workflow_id).await?;
        }
        Ok(())
    }

    fn parse_job_url(url: &Url) -> Result<JobUrl, Error> {
        if url.domain() != Some("circleci.com") {
            return Ok(JobUrl::Unrelated);
        }
        let segments: Vec<_> = url
            .path_segments()
            .ok_or_else(|| Error::Generic("invalid URL".into()))?
            .collect();
        if segments.len() != 4 {
            return Err(Error::Generic("invalid URL".into()));
        }
        let job_id = segments[3]
            .parse()
            .map_err(|_| Error::Generic("invalid job id".into()))?;
        Ok(JobUrl::Job {
            owner: segments[1],
            repo: segments[2],
            job_id,
        })
    }
}

enum JobUrl<'a> {
    Job {
        owner: &'a str,
        repo: &'a str,
        job_id: u64,
    },
    Unrelated,
}
