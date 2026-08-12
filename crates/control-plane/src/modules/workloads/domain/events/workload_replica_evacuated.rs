use crate::modules::shared_kernel::domain::{
    NodeCommandId, NodeId, OrganizationId, WorkloadId, WorkloadReplicaId,
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
pub struct WorkloadReplicaEvacuated {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub previous_replica_generation: u64,
    pub replica_generation: u64,
    pub ordinal: u32,
    pub source_node_id: NodeId,
    pub placement_generation: u64,
    pub runtime_fence_command_id: NodeCommandId,
    pub runtime_fenced_at: DateTime<Utc>,
    pub evacuated_at: DateTime<Utc>,
}

impl WorkloadReplicaEvacuated {
    pub fn envelope(
        previous: &WorkloadReplica,
        current: &WorkloadReplica,
        previous_member: &WorkloadReplicaMember,
        current_member: &WorkloadReplicaMember,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let source_node_id = previous
            .evacuation_node_id
            .ok_or_else(|| "Workload replica evacuation omitted its source node".to_string())?;
        let runtime_fence_command_id = previous
            .retirement_command_id
            .ok_or_else(|| "Workload replica evacuation omitted its Runtime fence".to_string())?;
        let runtime_fenced_at = previous.runtime_fenced_at.ok_or_else(|| {
            "Workload replica evacuation omitted its Runtime fencing time".to_string()
        })?;
        if previous.id != current.id
            || previous.organization_id != current.organization_id
            || previous.workload_id != current.workload_id
            || previous.ordinal != current.ordinal
            || previous.revision_id != current.revision_id
            || previous.revision_generation != current.revision_generation
            || previous.lifecycle != WorkloadReplicaLifecycle::Retiring
            || current.lifecycle != WorkloadReplicaLifecycle::Desired
            || previous.generation.checked_add(1) != Some(current.generation)
            || previous.aggregate_version.checked_add(1) != Some(current.aggregate_version)
            || current.evacuation_node_id.is_some()
            || current.retirement_command_id.is_some()
            || current.runtime_fenced_at.is_some()
            || previous_member.id != current_member.id
            || previous_member.replica_id != previous.id
            || current_member.replica_id != current.id
            || previous_member.node_id != Some(source_node_id)
            || current_member.node_id.is_some()
            || previous_member.placement_generation != current_member.placement_generation
            || previous_member.aggregate_version.checked_add(1)
                != Some(current_member.aggregate_version)
            || current.updated_at < previous.updated_at.max(current_member.updated_at)
            || correlation_id.is_nil()
        {
            return Err("Workload replica evacuated event has an invalid transition".into());
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.replica.evacuated".into(),
            schema_version: 1,
            organization_id: current.organization_id.as_uuid(),
            aggregate_id: current.id.as_uuid(),
            aggregate_version: current.aggregate_version,
            occurred_at: current.updated_at,
            correlation_id,
            causation_id: Some(runtime_fence_command_id.as_uuid()),
            payload: serde_json::to_value(Self {
                organization_id: current.organization_id,
                workload_id: current.workload_id,
                replica_id: current.id,
                previous_replica_generation: previous.generation,
                replica_generation: current.generation,
                ordinal: current.ordinal,
                source_node_id,
                placement_generation: previous_member.placement_generation,
                runtime_fence_command_id,
                runtime_fenced_at,
                evacuated_at: current.updated_at,
            })
            .map_err(|error| {
                format!("could not encode Workload replica evacuated event: {error}")
            })?,
        })
    }
}
