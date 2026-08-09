use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartWorkflowRunRequest {
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelWorkflowRunRequest {
    pub reason: Option<String>,
}
