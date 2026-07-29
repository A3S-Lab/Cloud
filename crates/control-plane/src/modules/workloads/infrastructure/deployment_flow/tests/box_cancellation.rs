use super::*;
#[cfg(target_os = "linux")]
use a3s_cloud_node_agent::build_box_runtime_client;
use a3s_cloud_node_agent::{
    BoxRuntimeConfig, CommandExecutor, FileCommandJournal, NodeResourceInventoryAuthority,
    ResourceInventoryError,
};
use a3s_runtime::contract::{RuntimeInspection, RuntimeUnitState};
#[cfg(not(target_os = "linux"))]
use a3s_runtime::RuntimeError;
use a3s_runtime::{RuntimeClient, RuntimeResult};
use async_trait::async_trait;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

type BoxTestResult<T> = Result<T, Box<dyn Error>>;

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_BOX=1 on the dedicated real Box provider runner"]
async fn real_box_deployment_cancellation_removes_runtime_before_claim_release() -> BoxTestResult<()>
{
    require_real_box_gate()?;
    let home = dedicated_box_home()?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let runtime = build_test_box_runtime(
        &BoxRuntimeConfig {
            home_dir: home.clone(),
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
        },
        runtime_state.path(),
    )?;
    let capabilities = runtime.capabilities().await?;

    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, registered_capabilities) = ready_node_with_capabilities(
        &nodes,
        organization_id,
        base,
        "real-box-cancellation",
        'b',
        500,
        128 * 1024 * 1024,
        capabilities,
    )
    .await?;
    let inventory = nodes
        .current_resource_inventory(node_id)
        .await?
        .ok_or("real Box node inventory is missing")?
        .inventory;
    let resource_claims = Arc::new(InMemoryResourceClaimRepository::new());
    let flow_runtime = runtime_with_resource_claims(
        &workloads,
        &nodes,
        resource_claims.clone(),
        Duration::seconds(240),
    )?;
    let engine = FlowEngine::in_memory(Arc::new(flow_runtime));
    let bundle = deployment_bundle_with_template(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("real Box cancellation")?,
            base,
        ),
        1,
        headless_box_service()?,
        base,
        "real-box-cancellation",
    )?;
    let deployment = bundle.deployment.clone();
    let revision = bundle.revision.clone();
    let operation = bundle.operation.clone();
    let spec = project_runtime_spec(&revision)?;
    workloads.create_deployment(bundle).await?;

    let journal = FileCommandJournal::new(node_state.path(), node_id.as_uuid())?;
    let executor = CommandExecutor::runtime_only(journal.clone(), runtime.clone())
        .with_resource_inventory(Arc::new(FixedInventory(inventory)));

    engine
        .start_with_id(
            operation.id.to_string(),
            workflow_spec(),
            operation.input.clone(),
        )
        .await?;
    let prepare = only_command(
        lease_for(&nodes, node_id, agent_instance_id, 0, Duration::minutes(3)).await?,
        "resource Claim prepare",
    )?;
    if !matches!(
        prepare.payload,
        NodeCommandPayload::ResourceClaimPrepare { .. }
    ) {
        return Err(invalid("first deployment command is not resource Claim prepare").into());
    }
    execute_and_deliver(
        &executor,
        &nodes,
        agent_instance_id,
        &registered_capabilities,
        &prepare,
    )
    .await?;

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let apply = only_command(
        lease_for(
            &nodes,
            node_id,
            agent_instance_id,
            prepare.sequence,
            Duration::minutes(3),
        )
        .await?,
        "resource-bound Runtime apply",
    )?;
    if !matches!(
        apply.payload,
        NodeCommandPayload::RuntimeApply {
            resource_claim: Some(_),
            ..
        }
    ) {
        return Err(invalid("second deployment command is not a bound Runtime apply").into());
    }
    let apply_ack = execute_and_deliver(
        &executor,
        &nodes,
        agent_instance_id,
        &registered_capabilities,
        &apply,
    )
    .await?;
    let applied = applied_observation(&apply_ack)?;
    if applied.state != RuntimeUnitState::Running || applied.unit_id != spec.unit_id {
        return Err(invalid("real Box apply did not create the exact running Service").into());
    }

    let applying = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    if applying.status != DeploymentStatus::Applying {
        return Err(invalid("deployment advanced before cancellation was requested").into());
    }
    workloads
        .mark_cancellation_requested(
            deployment.id,
            applying.aggregate_version,
            Utc::now().max(applying.updated_at),
        )
        .await?;

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;
    let removal = only_command(
        lease_for(
            &nodes,
            node_id,
            agent_instance_id,
            apply.sequence,
            Duration::minutes(3),
        )
        .await?,
        "Runtime removal",
    )?;
    if !matches!(removal.payload, NodeCommandPayload::RuntimeRemove { .. }) {
        return Err(invalid("cancellation did not dispatch Runtime remove").into());
    }
    let removal_ack = execute_and_deliver(
        &executor,
        &nodes,
        agent_instance_id,
        &registered_capabilities,
        &removal,
    )
    .await?;
    expect_removed(&removal_ack, &spec.unit_id, spec.generation)?;

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(3))
        .await?;
    let release = only_command(
        lease_for(
            &nodes,
            node_id,
            agent_instance_id,
            removal.sequence,
            Duration::minutes(3),
        )
        .await?,
        "resource Claim release",
    )?;
    if !matches!(
        release.payload,
        NodeCommandPayload::ResourceClaimRelease { .. }
    ) {
        return Err(invalid("Runtime removal was not followed by resource Claim release").into());
    }
    execute_and_deliver(
        &executor,
        &nodes,
        agent_instance_id,
        &registered_capabilities,
        &release,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(4))
        .await?;

    if !(prepare.sequence < apply.sequence
        && apply.sequence < removal.sequence
        && removal.sequence < release.sequence)
    {
        return Err(
            invalid("deployment cleanup command ordering is not strictly monotonic").into(),
        );
    }
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    if cancelled.status != DeploymentStatus::Cancelled {
        return Err(invalid("deployment cancellation did not become terminal").into());
    }
    if resource_claims
        .find(
            organization_id,
            ResourceClaimId::from_uuid(deployment.id.as_uuid()),
        )
        .await?
        .state
        != crate::modules::workloads::domain::entities::ResourceClaimState::Released
    {
        return Err(invalid("deployment resource Claim was not released").into());
    }
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("removed Box Service remained inspectable").into());
    }
    verify_empty_box_state_and_remove_fixture_files(&home)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn build_test_box_runtime(
    config: &BoxRuntimeConfig,
    state_root: &Path,
) -> RuntimeResult<Arc<dyn RuntimeClient>> {
    build_box_runtime_client(config, state_root)
}

#[cfg(not(target_os = "linux"))]
fn build_test_box_runtime(
    _config: &BoxRuntimeConfig,
    _state_root: &Path,
) -> RuntimeResult<Arc<dyn RuntimeClient>> {
    Err(RuntimeError::ProviderUnavailable(
        "real Box deployment cancellation validation requires Linux".into(),
    ))
}

async fn execute_and_deliver(
    executor: &CommandExecutor,
    nodes: &InMemoryNodeRepository,
    agent_instance_id: Uuid,
    capabilities: &a3s_runtime::contract::RuntimeCapabilities,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
) -> BoxTestResult<NodeCommandAck> {
    let acknowledgement = executor.execute(command.clone()).await?;
    if let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome {
        if let a3s_cloud_contracts::NodeCommandResult::RuntimeApplied { observation } =
            result.as_ref()
        {
            record_observation(
                nodes,
                NodeId::from_uuid(command.node_id),
                agent_instance_id,
                capabilities,
                command,
                observation.as_ref().clone(),
            )
            .await?;
        }
    }
    nodes
        .acknowledge_command(acknowledgement.clone(), Utc::now())
        .await?;
    Ok(acknowledgement)
}

fn only_command(
    lease: a3s_cloud_contracts::NodeCommandLeaseResponse,
    label: &str,
) -> BoxTestResult<a3s_cloud_contracts::NodeCommandEnvelope> {
    if lease.commands.len() != 1 {
        return Err(invalid(format!(
            "expected one {label} command, received {}",
            lease.commands.len()
        ))
        .into());
    }
    lease
        .commands
        .into_iter()
        .next()
        .ok_or_else(|| invalid(format!("missing {label} command")).into())
}

fn headless_box_service() -> BoxTestResult<ServiceTemplate> {
    let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let (repository, digest_hex) = image
        .rsplit_once("@sha256:")
        .ok_or_else(|| invalid("Box conformance image is not digest-pinned"))?;
    if repository.is_empty()
        || digest_hex.len() != 64
        || !digest_hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("Box conformance image digest is invalid").into());
    }
    let digest = format!("sha256:{digest_hex}");
    Ok(ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://{repository}@{digest}"),
            digest,
            media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
        },
        process: ServiceProcess {
            command: vec!["/bin/sh".into(), "-c".into()],
            args: vec!["printf 'cloud-box-cancellation-ready\\n'; exec sleep 3600".into()],
            working_directory: Some("/".into()),
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: Vec::new(),
        health: None,
    })
}

fn applied_observation(
    acknowledgement: &NodeCommandAck,
) -> BoxTestResult<&a3s_runtime::contract::RuntimeObservation> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            a3s_cloud_contracts::NodeCommandResult::RuntimeApplied { observation } => {
                Ok(observation)
            }
            result => Err(invalid(format!("unexpected apply result: {result:?}")).into()),
        },
        outcome => Err(invalid(format!("Runtime apply did not succeed: {outcome:?}")).into()),
    }
}

fn expect_removed(
    acknowledgement: &NodeCommandAck,
    unit_id: &str,
    generation: u64,
) -> BoxTestResult<()> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            a3s_cloud_contracts::NodeCommandResult::RuntimeRemoved { removal }
                if removal.unit_id == unit_id && removal.generation == generation =>
            {
                Ok(())
            }
            result => Err(invalid(format!("unexpected remove result: {result:?}")).into()),
        },
        outcome => Err(invalid(format!("Runtime remove did not succeed: {outcome:?}")).into()),
    }
}

struct FixedInventory(NodeResourceInventory);

#[async_trait]
impl NodeResourceInventoryAuthority for FixedInventory {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        Ok(self.0.clone())
    }
}

fn require_real_box_gate() -> BoxTestResult<()> {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(invalid("dedicated Box gate did not set A3S_CLOUD_TEST_BOX=1").into());
    }
    Ok(())
}

fn dedicated_box_home() -> BoxTestResult<PathBuf> {
    let configured = PathBuf::from(
        std::env::var_os("A3S_HOME")
            .ok_or_else(|| invalid("dedicated Box gate did not configure an absolute A3S_HOME"))?,
    );
    if !configured.is_absolute() {
        return Err(invalid("dedicated Box gate A3S_HOME is not absolute").into());
    }
    let canonical = configured.canonicalize()?;
    if canonical != configured {
        return Err(invalid("dedicated Box gate A3S_HOME is not canonical").into());
    }
    Ok(configured)
}

fn verify_empty_box_state_and_remove_fixture_files(home: &Path) -> BoxTestResult<()> {
    let state_path = home.join("boxes.json");
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(&state_path)?)?;
    if !state.as_array().is_some_and(Vec::is_empty) {
        return Err(invalid("Box retained managed execution state after cancellation").into());
    }
    for path in [
        state_path,
        home.join("boxes.json.lock"),
        home.join("boxes.json.tmp"),
    ] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
