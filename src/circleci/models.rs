use serde_derive::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Job {
    pub name: String,
    pub latest_workflow: WorkflowSummary,
    pub status: JobStatus,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct WorkflowSummary {
    pub id: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum JobStatus {
    #[serde(rename = "failed")]
    Failed,

    #[serde(rename = "success")]
    Success,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct NoBody {}
