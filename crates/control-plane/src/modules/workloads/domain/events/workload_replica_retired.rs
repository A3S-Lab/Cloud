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
pub struct WorkloadReplicaRetired {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub ordinal: u32,
    pub node_id: Option<NodeId>,
    pub placement_generation: u64,
    pub runtime_fence_command_id: Option<NodeCommandId>,
    pub runtime_fenced_at: Option<DateTime<Utc>>,
    pub retired_at: DateTime<Utc>,
}

impl WorkloadReplicaRetired {
    pub fn envelope(
        previous: &WorkloadReplica,
        current: &WorkloadReplica,
        previous_member: &WorkloadReplicaMember,
        current_member: &WorkloadReplicaMember,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let expected_member_version = previous_member
            .aggregate_version
            .checked_add(u64::from(previous_member.node_id.is_some()))
            .ok_or_else(|| "Workload replica member version overflowed".to_string())?;
        if previous.id != current.id
            || previous.organization_id != current.organization_id
            || previous.workload_id != current.workload_id
            || previous.generation != current.generation
            || previous.ordinal != current.ordinal
            || previous.lifecycle != WorkloadReplicaLifecycle::Retiring
            || previous.evacuation_node_id.is_some()
            || current.lifecycle != WorkloadReplicaLifecycle::Retired
            || current.evacuation_node_id.is_some()
            || previous.aggregate_version.checked_add(1) != Some(current.aggregate_version)
            || previous.retirement_command_id != current.retirement_command_id
            || previous.runtime_fenced_at != current.runtime_fenced_at
            || previous.retirement_command_id.is_some() != previous.runtime_fenced_at.is_some()
            || previous_member.node_id.is_some() && previous.runtime_fenced_at.is_none()
            || previous_member.id != current_member.id
            || previous_member.replica_id != previous.id
            || current_member.replica_id != current.id
            || previous_member.placement_generation != current_member.placement_generation
            || current_member.node_id.is_some()
            || current_member.aggregate_version != expected_member_version
            || current.updated_at < previous.updated_at.max(current_member.updated_at)
            || correlation_id.is_nil()
        {
            return Err("Workload replica retirement event has an invalid transition".into());
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.replica.retired".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: current.organization_id.as_uuid(),
            },
            aggregate_id: current.id.as_uuid(),
            aggregate_version: current.aggregate_version,
            occurred_at: current.updated_at,
            correlation_id,
            causation_id: previous.retirement_command_id.map(NodeCommandId::as_uuid),
            payload: serde_json::to_value(Self {
                organization_id: current.organization_id,
                workload_id: current.workload_id,
                replica_id: current.id,
                replica_generation: current.generation,
                ordinal: current.ordinal,
                node_id: previous_member.node_id,
                placement_generation: previous_member.placement_generation,
                runtime_fence_command_id: current.retirement_command_id,
                runtime_fenced_at: current.runtime_fenced_at,
                retired_at: current.updated_at,
            })
            .map_err(|error| {
                format!("could not encode Workload replica retirement event: {error}")
            })?,
        })
    }
}
