use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunHistoryEvent {
    pub sequence: u64,
    pub event_id: uuid::Uuid,
    pub event_key: String,
    pub occurred_at: DateTime<Utc>,
    pub step_id: Option<String>,
    pub attempt: Option<u32>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunHistoryPage {
    pub events: Vec<WorkflowRunHistoryEvent>,
    pub next_sequence: Option<u64>,
}

#[async_trait]
pub trait IWorkflowRunHistoryReader: Send + Sync {
    async fn read(
        &self,
        flow_run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<WorkflowRunHistoryPage, String>;
}
