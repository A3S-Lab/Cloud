use crate::modules::shared_kernel::domain::{
    NodeId, OrganizationId, WorkloadId, WorkloadReplicaId,
};
use crate::modules::workloads::domain::entities::{
    WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadReplicaEvacuationRequested {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub ordinal: u32,
    pub source_node_id: NodeId,
    pub placement_generation: u64,
    pub requested_at: DateTime<Utc>,
}

impl WorkloadReplicaEvacuationRequested {
    pub fn envelope(
        previous: &WorkloadReplica,
        current: &WorkloadReplica,
        member: &WorkloadReplicaMember,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let source_node_id = current
            .evacuation_node_id
            .ok_or_else(|| "Workload replica evacuation omitted its source node".to_string())?;
        if previous.id != current.id
            || previous.organization_id != current.organization_id
            || previous.workload_id != current.workload_id
            || previous.generation != current.generation
            || previous.ordinal != current.ordinal
            || previous.lifecycle != WorkloadReplicaLifecycle::Desired
            || previous.evacuation_node_id.is_some()
            || previous.retirement_command_id.is_some()
            || previous.runtime_fenced_at.is_some()
            || current.lifecycle != WorkloadReplicaLifecycle::Retiring
            || current.retirement_command_id.is_some()
            || current.runtime_fenced_at.is_some()
            || previous.aggregate_version.checked_add(1) != Some(current.aggregate_version)
            || member.organization_id != current.organization_id
            || member.workload_id != current.workload_id
            || member.replica_id != current.id
            || member.node_id != Some(source_node_id)
            || member.placement_generation == 0
            || current.updated_at < previous.updated_at.max(member.updated_at)
            || correlation_id.is_nil()
        {
            return Err(
                "Workload replica evacuation request event has an invalid transition".into(),
            );
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.replica.evacuation.requested".into(),
            schema_version: 1,
            organization_id: current.organization_id.as_uuid(),
            aggregate_id: current.id.as_uuid(),
            aggregate_version: current.aggregate_version,
            occurred_at: current.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: current.organization_id,
                workload_id: current.workload_id,
                replica_id: current.id,
                replica_generation: current.generation,
                ordinal: current.ordinal,
                source_node_id,
                placement_generation: member.placement_generation,
                requested_at: current.updated_at,
            })
            .map_err(|error| {
                format!("could not encode Workload replica evacuation request event: {error}")
            })?,
        })
    }
}
