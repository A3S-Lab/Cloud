use crate::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::Deployment;
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentCancellationRequested {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub requested_at: DateTime<Utc>,
}

impl DeploymentCancellationRequested {
    pub fn envelope(
        deployment: &Deployment,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let requested_at = deployment
            .cancellation_requested_at
            .unwrap_or(deployment.updated_at);
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.deployment.cancellation-requested".into(),
            schema_version: 1,
            organization_id: deployment.organization_id.as_uuid(),
            aggregate_id: deployment.id.as_uuid(),
            aggregate_version: deployment.aggregate_version,
            occurred_at: requested_at,
            correlation_id,
            causation_id: Some(deployment.operation_id.as_uuid()),
            payload: serde_json::to_value(Self {
                organization_id: deployment.organization_id,
                workload_id: deployment.workload_id,
                revision_id: deployment.revision_id,
                deployment_id: deployment.id,
                operation_id: deployment.operation_id,
                requested_at,
            })?,
        })
    }
}
