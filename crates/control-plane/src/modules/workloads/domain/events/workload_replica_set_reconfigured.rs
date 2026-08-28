use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId};
use crate::modules::workloads::domain::entities::WorkloadControl;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadReplicaSetReconfigured {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub previous_policy_generation: u64,
    pub policy_generation: u64,
    pub previous_desired_replicas: u32,
    pub desired_replicas: u32,
}

impl WorkloadReplicaSetReconfigured {
    pub fn envelope(
        previous: &WorkloadControl,
        current: &WorkloadControl,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let previous_generation = previous.spec.placement_policy.generation();
        let current_generation = current.spec.placement_policy.generation();
        if previous.organization_id != current.organization_id
            || previous.project_id != current.project_id
            || previous.environment_id != current.environment_id
            || previous.workload_id != current.workload_id
            || previous.aggregate_version.checked_add(1) != Some(current.aggregate_version)
            || previous_generation.checked_add(1) != Some(current_generation)
            || previous.spec.placement_policy.desired_replicas()
                == current.spec.placement_policy.desired_replicas()
            || correlation_id.is_nil()
        {
            return Err("Workload replica-set event has an invalid transition".into());
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.replica-set.reconfigured".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: current.organization_id.as_uuid(),
            },
            aggregate_id: current.workload_id.as_uuid(),
            aggregate_version: current.aggregate_version,
            occurred_at: current.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: current.organization_id,
                workload_id: current.workload_id,
                previous_policy_generation: previous_generation,
                policy_generation: current_generation,
                previous_desired_replicas: previous.spec.placement_policy.desired_replicas(),
                desired_replicas: current.spec.placement_policy.desired_replicas(),
            })
            .map_err(|error| format!("could not encode Workload replica-set event: {error}"))?,
        })
    }
}
