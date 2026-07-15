use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::{
    OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::Workload;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadStopRequested {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub active_revision_id: Option<WorkloadRevisionId>,
    pub operation_id: OperationId,
}

impl WorkloadStopRequested {
    pub fn envelope(
        workload: &Workload,
        operation: &OperationRequest,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.stop.requested".into(),
            schema_version: 1,
            organization_id: workload.organization_id.as_uuid(),
            aggregate_id: workload.id.as_uuid(),
            aggregate_version: workload.aggregate_version,
            occurred_at: operation.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: workload.organization_id,
                workload_id: workload.id,
                active_revision_id: workload.active_revision_id,
                operation_id: operation.id,
            })?,
        })
    }
}
