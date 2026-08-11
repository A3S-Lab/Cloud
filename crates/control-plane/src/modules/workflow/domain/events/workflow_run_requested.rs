use crate::modules::workflow::domain::WorkflowRun;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunRequested {
    pub project_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub operation_id: Uuid,
    pub flow_run_id: String,
    pub execution_input_digest: String,
    pub deadline_at: chrono::DateTime<chrono::Utc>,
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
            aggregate_version: run.aggregate_version,
            occurred_at: run.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                project_id: run.project_id.as_uuid(),
                workflow_run_id: run.id.as_uuid(),
                workflow_goal_id: run.workflow_goal_id.as_uuid(),
                plan_revision_id: run.plan_revision_id.as_uuid(),
                plan_digest: run.plan_digest.as_str().to_owned(),
                operation_id: run.operation_id.as_uuid(),
                flow_run_id: run.flow_run_id.clone(),
                execution_input_digest: run.execution_input_digest.as_str().to_owned(),
                deadline_at: run.execution_input.deadline_at,
            })?,
        })
    }
}
