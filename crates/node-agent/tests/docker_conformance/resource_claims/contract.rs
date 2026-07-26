use crate::fixture::{require, DockerConformanceFixture};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeResourceClaimBinding, NodeResourceInventory,
    NodeResourceSlot, ResourceAllocation, ResourceKind, ResourceSlotBinding, ResourceUnit,
};
use a3s_cloud_node_agent::{NodeResourceInventoryAuthority, ResourceInventoryError};
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeObservation, RuntimeUnitSpec};
use a3s_runtime::{
    FileRuntimeStateStore, RuntimeError, RuntimeRequestState, RuntimeResult, RuntimeStateStore,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn command(
    node_id: Uuid,
    aggregate_id: Uuid,
    sequence: u64,
    generation: u64,
    payload: NodeCommandPayload,
) -> RuntimeResult<NodeCommandEnvelope> {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + Duration::minutes(10),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(RuntimeError::Protocol)
    .and_then(|command| {
        require(
            command.generation == generation,
            "resource Claim command generation does not match its payload",
        )?;
        Ok(command)
    })
}

pub(super) fn claim_inventory(
    node_id: Uuid,
    agent_instance_id: Uuid,
    spec: &RuntimeUnitSpec,
) -> RuntimeResult<NodeResourceInventory> {
    NodeResourceInventory::new(
        node_id,
        agent_instance_id,
        1,
        Utc::now(),
        vec![
            NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/claim-crash",
                ResourceAllocation::Scalar {
                    amount: spec.resources.cpu_millis,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            .map_err(RuntimeError::Protocol)?,
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/claim-crash",
                ResourceAllocation::Scalar {
                    amount: spec.resources.memory_bytes,
                    unit: ResourceUnit::Byte,
                },
            )
            .map_err(RuntimeError::Protocol)?,
        ],
    )
    .map_err(RuntimeError::Protocol)
}

pub(super) fn claim_binding(
    claim_id: Uuid,
    inventory: &NodeResourceInventory,
    runtime_unit_id: &str,
    runtime_generation: u64,
    spec: &RuntimeUnitSpec,
) -> NodeResourceClaimBinding {
    NodeResourceClaimBinding {
        schema: NodeResourceClaimBinding::SCHEMA.into(),
        claim_id,
        node_id: inventory.node_id,
        agent_instance_id: inventory.agent_instance_id,
        inventory_generation: inventory.generation,
        inventory_digest: inventory.digest.clone(),
        runtime_unit_id: runtime_unit_id.into(),
        runtime_generation,
        topology_digest: sha256('a'),
        slots: vec![
            ResourceSlotBinding {
                kind: ResourceKind::Cpu,
                stable_resource_id: "cpu/claim-crash".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.cpu_millis,
                    unit: ResourceUnit::MilliCpu,
                },
                slot_generation: 1,
                fence_token: Uuid::now_v7(),
            },
            ResourceSlotBinding {
                kind: ResourceKind::Memory,
                stable_resource_id: "memory/claim-crash".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.memory_bytes,
                    unit: ResourceUnit::Byte,
                },
                slot_generation: 1,
                fence_token: Uuid::now_v7(),
            },
        ],
    }
}

pub(super) fn inventory_for_binding(
    binding: &NodeResourceClaimBinding,
) -> RuntimeResult<NodeResourceInventory> {
    let slots = binding
        .slots
        .iter()
        .map(|slot| {
            NodeResourceSlot::new(
                slot.kind,
                slot.stable_resource_id.clone(),
                slot.allocation.clone(),
            )
            .map_err(RuntimeError::Protocol)
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    let inventory = NodeResourceInventory::new(
        binding.node_id,
        binding.agent_instance_id,
        binding.inventory_generation,
        Utc::now(),
        slots,
    )
    .map_err(RuntimeError::Protocol)?;
    require(
        inventory.digest == binding.inventory_digest,
        "resource Claim crash probe reconstructed a different inventory digest",
    )?;
    Ok(inventory)
}

pub(super) fn inventory_authority(
    inventory: NodeResourceInventory,
) -> Arc<dyn NodeResourceInventoryAuthority> {
    Arc::new(FixedInventoryAuthority { inventory })
}

pub(super) fn runtime_action(prefix: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("{prefix}-{}", Uuid::now_v7()),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

pub(super) async fn require_pending_apply(
    store: &Arc<FileRuntimeStateStore>,
    command: &NodeCommandEnvelope,
) -> RuntimeResult<()> {
    let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
        return Err(RuntimeError::Protocol(
            "resource Claim crash command is not Runtime apply".into(),
        ));
    };
    let receipt = store
        .load_request(&request.spec.unit_id, &request.request_id)
        .await?;
    require(
        receipt.state == RuntimeRequestState::Pending && receipt.observation.is_none(),
        "resource Claim crash did not preserve an ambiguous pending Runtime apply",
    )
}

pub(super) async fn require_exact_unit(
    fixture: &DockerConformanceFixture,
    unit_id: &str,
    expected_id: &str,
    phase: &str,
) -> RuntimeResult<()> {
    let ids = fixture.unit_container_ids(unit_id).await?;
    require(
        ids == [expected_id],
        format!("{phase} expected exactly provider unit {expected_id:?}, found {ids:?}"),
    )
}

pub(super) fn applied_observation(
    acknowledgement: &NodeCommandAck,
) -> RuntimeResult<&RuntimeObservation> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
            other => Err(RuntimeError::Protocol(format!(
                "resource Claim apply returned unexpected result {other:?}"
            ))),
        },
        outcome => Err(RuntimeError::Protocol(format!(
            "resource Claim apply returned non-success outcome {outcome:?}"
        ))),
    }
}

pub(super) fn require_success(
    acknowledgement: &NodeCommandAck,
    operation: &str,
) -> RuntimeResult<()> {
    require(
        matches!(
            acknowledgement.outcome,
            NodeCommandOutcome::Succeeded { .. }
        ),
        format!("{operation} returned {:?}", acknowledgement.outcome),
    )
}

pub(super) fn require_rejected(
    acknowledgement: &NodeCommandAck,
    code: &str,
    message_fragment: &str,
) -> RuntimeResult<()> {
    require(
        matches!(
            &acknowledgement.outcome,
            NodeCommandOutcome::Rejected { failure }
                if failure.code == code && failure.message.contains(message_fragment)
        ),
        format!(
            "resource Claim fence returned {:?}, expected {code:?} containing {message_fragment:?}",
            acknowledgement.outcome
        ),
    )
}

pub(super) fn sha256(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

pub(super) fn fixture_error(operation: &str, error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::ProviderUnavailable(format!("{operation} failed: {error}"))
}

struct FixedInventoryAuthority {
    inventory: NodeResourceInventory,
}

#[async_trait]
impl NodeResourceInventoryAuthority for FixedInventoryAuthority {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        Ok(self.inventory.clone())
    }
}
