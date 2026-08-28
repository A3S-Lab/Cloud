use crate::modules::executions::domain::Execution;
use crate::modules::shared_kernel::domain::{ExecutionId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionCancellationRequested {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
}

impl ExecutionCancellationRequested {
    pub fn envelope(
        execution: &Execution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "execution.run.cancellation-requested".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: execution.organization_id.as_uuid(),
            },
            aggregate_id: execution.id.as_uuid(),
            aggregate_version: execution.aggregate_version,
            occurred_at: execution.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: execution.organization_id,
                execution_id: execution.id,
            })?,
        })
    }
}
