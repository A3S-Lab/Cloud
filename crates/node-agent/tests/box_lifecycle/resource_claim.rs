use super::{
    action_request, applied_observation, command, expect_not_found, expect_removed, expect_stopped,
    invalid, runtime, runtime_spec, succeeded_result, wait_for_log, TestResult,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeResourceClaimBinding, NodeResourceClaimPrepare, NodeResourceClaimRelease,
    NodeResourceInventory, NodeResourceSlot, ResourceAllocation, ResourceKind, ResourceSlotBinding,
    ResourceUnit,
};
use a3s_cloud_node_agent::{
    CommandExecutor, FileCommandJournal, NodeResourceInventoryAuthority, ResourceInventoryError,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeInspection, RuntimeUnitClass, RuntimeUnitState};
use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

pub(super) async fn prove_resource_claim_lifecycle(
    home: &Path,
    runtime_state: &TempDir,
    journal: &FileCommandJournal,
    node_id: Uuid,
    artifact: ArtifactRef,
) -> TestResult<()> {
    let claim_id = Uuid::now_v7();
    let workload_id = Uuid::now_v7();
    let spec = runtime_spec(
        artifact,
        format!("cloud-box-claim-{}", Uuid::now_v7().simple()),
        1,
        RuntimeUnitClass::Service,
        "printf 'cloud-box-claim-ready\\n'; exec sleep 3600",
    )?;
    let inventory = resource_inventory(node_id, Uuid::now_v7(), &spec)?;
    let binding = resource_binding(claim_id, &inventory, &spec)?;
    let authority: Arc<dyn NodeResourceInventoryAuthority> =
        Arc::new(GateResourceInventory::new(inventory));
    let prepare = NodeResourceClaimPrepare {
        schema: NodeResourceClaimPrepare::SCHEMA.into(),
        claim_generation: 1,
        claim_digest: digest(&format!("prepare:{claim_id}")),
        binding: binding.clone(),
    };
    prepare.validate().map_err(invalid)?;

    let preparing_runtime = runtime(home, runtime_state.path())?;
    let preparing_executor = CommandExecutor::runtime_only(journal.clone(), preparing_runtime)
        .with_resource_inventory(authority.clone());
    let prepared = preparing_executor
        .execute(command(
            node_id,
            claim_id,
            8,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(prepare.clone()),
            },
        )?)
        .await?;
    expect_prepared(&prepared, &prepare)?;
    drop(preparing_executor);

    let applying_runtime = runtime(home, runtime_state.path())?;
    let applying_executor =
        CommandExecutor::runtime_only(journal.clone(), applying_runtime.clone())
            .with_resource_inventory(authority.clone());
    let applied = applying_executor
        .execute(command(
            node_id,
            workload_id,
            9,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(super::apply_request("cloud-box-claim-apply", spec.clone())),
                resource_claim: Some(Box::new(binding.clone())),
            },
        )?)
        .await?;
    let observation = applied_observation(&applied)?;
    if observation.state != RuntimeUnitState::Running {
        return Err(invalid("resource-bound Box Service did not become running").into());
    }
    binding
        .validate_runtime_observation(observation)
        .map_err(invalid)?;
    wait_for_log(&*applying_runtime, &spec, "cloud-box-claim-ready").await?;
    drop(applying_executor);
    drop(applying_runtime);

    let inspecting_runtime = runtime(home, runtime_state.path())?;
    let inspecting_executor =
        CommandExecutor::runtime_only(journal.clone(), inspecting_runtime.clone())
            .with_resource_inventory(authority.clone());
    let inspected = inspecting_executor
        .execute(command(
            node_id,
            workload_id,
            10,
            NodeCommandPayload::RuntimeInspect {
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
            },
        )?)
        .await?;
    expect_bound_inspection(&inspected, &binding)?;

    let early_release_request = NodeResourceClaimRelease {
        schema: NodeResourceClaimRelease::SCHEMA.into(),
        claim_generation: 2,
        claim_digest: digest(&format!("early-release:{claim_id}")),
        binding: binding.clone(),
    };
    let early_release = inspecting_executor
        .execute(command(
            node_id,
            claim_id,
            11,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(early_release_request),
            },
        )?)
        .await?;
    expect_release_fenced(&early_release)?;

    let stopped = inspecting_executor
        .execute(command(
            node_id,
            workload_id,
            12,
            NodeCommandPayload::RuntimeStop {
                request: action_request("cloud-box-claim-stop", &spec),
            },
        )?)
        .await?;
    expect_stopped(&stopped)?;
    drop(inspecting_executor);
    drop(inspecting_runtime);

    let releasing_runtime = runtime(home, runtime_state.path())?;
    let releasing_executor =
        CommandExecutor::runtime_only(journal.clone(), releasing_runtime.clone())
            .with_resource_inventory(authority);
    let release_request = NodeResourceClaimRelease {
        schema: NodeResourceClaimRelease::SCHEMA.into(),
        claim_generation: 3,
        claim_digest: digest(&format!("release:{claim_id}")),
        binding,
    };
    let released = releasing_executor
        .execute(command(
            node_id,
            claim_id,
            13,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(release_request.clone()),
            },
        )?)
        .await?;
    expect_released(&released, &release_request)?;

    let removed = releasing_executor
        .execute(command(
            node_id,
            workload_id,
            14,
            NodeCommandPayload::RuntimeRemove {
                request: action_request("cloud-box-claim-remove", &spec),
            },
        )?)
        .await?;
    expect_removed(&removed)?;
    expect_not_found(&*releasing_runtime, &spec.unit_id).await?;
    Ok(())
}

fn resource_inventory(
    node_id: Uuid,
    agent_instance_id: Uuid,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> TestResult<NodeResourceInventory> {
    NodeResourceInventory::new(
        node_id,
        agent_instance_id,
        1,
        Utc::now(),
        vec![
            NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/shared",
                ResourceAllocation::Scalar {
                    amount: spec.resources.cpu_millis,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            .map_err(invalid)?,
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/system",
                ResourceAllocation::Scalar {
                    amount: spec.resources.memory_bytes,
                    unit: ResourceUnit::Byte,
                },
            )
            .map_err(invalid)?,
        ],
    )
    .map_err(|error| invalid(error).into())
}

fn resource_binding(
    claim_id: Uuid,
    inventory: &NodeResourceInventory,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> TestResult<NodeResourceClaimBinding> {
    let binding = NodeResourceClaimBinding {
        schema: NodeResourceClaimBinding::SCHEMA.into(),
        claim_id,
        node_id: inventory.node_id,
        agent_instance_id: inventory.agent_instance_id,
        inventory_generation: inventory.generation,
        inventory_digest: inventory.digest.clone(),
        runtime_unit_id: spec.unit_id.clone(),
        runtime_generation: spec.generation,
        topology_digest: digest("cloud-box-claim-topology"),
        slots: vec![
            ResourceSlotBinding {
                kind: ResourceKind::Cpu,
                stable_resource_id: "cpu/shared".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.cpu_millis,
                    unit: ResourceUnit::MilliCpu,
                },
                slot_generation: 1,
                fence_token: Uuid::now_v7(),
            },
            ResourceSlotBinding {
                kind: ResourceKind::Memory,
                stable_resource_id: "memory/system".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.memory_bytes,
                    unit: ResourceUnit::Byte,
                },
                slot_generation: 1,
                fence_token: Uuid::now_v7(),
            },
        ],
    };
    binding.validate().map_err(invalid)?;
    binding.validate_inventory(inventory).map_err(invalid)?;
    binding.validate_runtime_spec(spec).map_err(invalid)?;
    Ok(binding)
}

fn expect_prepared(
    acknowledgement: &NodeCommandAck,
    request: &NodeResourceClaimPrepare,
) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::ResourceClaimPrepared { prepared } => prepared
            .validate_for(request)
            .map_err(|error| invalid(error).into()),
        result => Err(invalid(format!(
            "unexpected resource Claim prepare result: {result:?}"
        ))
        .into()),
    }
}

fn expect_bound_inspection(
    acknowledgement: &NodeCommandAck,
    binding: &NodeResourceClaimBinding,
) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeInspected {
            inspection: RuntimeInspection::Found { observation, .. },
        } => binding
            .validate_runtime_observation(observation)
            .map_err(|error| invalid(error).into()),
        result => Err(invalid(format!("unexpected bound Runtime inspection: {result:?}")).into()),
    }
}

fn expect_release_fenced(acknowledgement: &NodeCommandAck) -> TestResult<()> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Rejected { failure }
            if failure.code == "resource_claim_journal" && !failure.retryable =>
        {
            Ok(())
        }
        outcome => Err(invalid(format!(
            "resource Claim release before Runtime stop was not fenced: {outcome:?}"
        ))
        .into()),
    }
}

fn expect_released(
    acknowledgement: &NodeCommandAck,
    request: &NodeResourceClaimRelease,
) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::ResourceClaimReleased { released } => released
            .validate_for(request)
            .map_err(|error| invalid(error).into()),
        result => Err(invalid(format!(
            "unexpected resource Claim release result: {result:?}"
        ))
        .into()),
    }
}

fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

#[derive(Debug, Clone)]
struct GateResourceInventory {
    inventory: NodeResourceInventory,
}

impl GateResourceInventory {
    fn new(inventory: NodeResourceInventory) -> Self {
        // The gate supplies the authenticated inventory snapshot consumed by
        // the existing Agent authority port; it does not reproduce detection
        // or persistence owned by ResourceInventoryManager.
        Self { inventory }
    }
}

#[async_trait]
impl NodeResourceInventoryAuthority for GateResourceInventory {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        Ok(self.inventory.clone())
    }
}
