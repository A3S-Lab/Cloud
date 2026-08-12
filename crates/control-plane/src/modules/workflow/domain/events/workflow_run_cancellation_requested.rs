use crate::modules::workflow::domain::WorkflowRun;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunCancellationRequested {
    pub project_id: Uuid,
    pub workflow_run_id: Uuid,
    pub operation_id: Uuid,
    pub flow_run_id: String,
    pub requested_by: Uuid,
    pub reason: Option<String>,
}

impl WorkflowRunCancellationRequested {
    pub fn envelope(
        run: &WorkflowRun,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let occurred_at = run
            .cancellation_requested_at
            .ok_or_else(|| "WorkflowRun cancellation event has no request time".to_owned())?;
        let requested_by = run.cancellation_requested_by.ok_or_else(|| {
            "WorkflowRun cancellation event has no requesting principal".to_owned()
        })?;
        let payload = serde_json::to_value(Self {
            project_id: run.project_id.as_uuid(),
            workflow_run_id: run.id.as_uuid(),
            operation_id: run.operation_id.as_uuid(),
            flow_run_id: run.flow_run_id.clone(),
            requested_by: requested_by.as_uuid(),
            reason: run.cancellation_reason.clone(),
        })
        .map_err(|error| error.to_string())?;
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workflow.run.cancellation.requested".into(),
            schema_version: 2,
            organization_id: run.organization_id.as_uuid(),
            aggregate_id: run.id.as_uuid(),
            aggregate_version: run.aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload,
        })
    }
}
