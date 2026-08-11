use crate::modules::workflow::domain::WorkflowRun;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunRequested {
    pub project_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub operation_id: Uuid,
}

impl WorkflowRunRequested {
    pub fn envelope(
        run: &WorkflowRun,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workflow.run.requested".into(),
            schema_version: 1,
            organization_id: run.organization_id.as_uuid(),
            aggregate_id: run.id.as_uuid(),
            aggregate_version: 1,
            occurred_at: run.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                project_id: run.project_id.as_uuid(),
                workflow_run_id: run.id.as_uuid(),
                workflow_goal_id: run.workflow_goal_id.as_uuid(),
                plan_revision_id: run.plan_revision_id.as_uuid(),
                plan_digest: run.plan_digest.to_string(),
                operation_id: run.operation_id.as_uuid(),
            })?,
        })
    }
}
