use crate::modules::executions::domain::Execution;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRequested {
    pub execution_id: crate::modules::shared_kernel::domain::ExecutionId,
    pub project_id: crate::modules::shared_kernel::domain::ProjectId,
    pub environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    pub template_digest: String,
}

impl ExecutionRequested {
    pub fn envelope(
        execution: &Execution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "execution.run.requested".into(),
            schema_version: 1,
            organization_id: execution.organization_id.as_uuid(),
            aggregate_id: execution.id.as_uuid(),
            aggregate_version: execution.aggregate_version,
            occurred_at: execution.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                execution_id: execution.id,
                project_id: execution.project_id,
                environment_id: execution.environment_id,
                template_digest: execution.template_digest.clone(),
            })?,
        })
    }
}
