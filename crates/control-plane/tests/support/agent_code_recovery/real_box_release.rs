use super::*;
use a3s_box_runtime::BoxStateStore;
use a3s_cloud_contracts::{
    agent_release_manifest_archive, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeResourceInventory, NodeResourceSlot, ResourceAllocation,
    ResourceKind, ResourceUnit, RuntimeObservationReport, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
    OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, OperationRequest,
    OperationStatus, OperationSubject, PostgresOperationRepository, ReconcileOperationsHandler,
    WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::OperationId;
use a3s_cloud_control_plane::modules::workloads::application::{
    STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentFlowConfig, DeploymentFlowDependencies, DeploymentFlowRuntime, DeploymentStatus,
    IOciArtifactResolver, OciArtifact, OciArtifactReference, OciArtifactResolutionError,
    OciRegistryCredentialReference, PostgresResourceClaimRepository, RequestWorkloadStopBundle,
    UnroutedDeploymentRouteUpdater, WorkloadDesiredState, WorkloadStopRequested,
};
use a3s_cloud_node_agent::{
    build_box_runtime_provider, ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation,
    CommandExecutor, DownloadedNodeArtifact, FileCommandJournal, NodeArtifactManager,
    NodeArtifactTransport, NodeControlClientError, NodeResourceInventoryAuthority,
    ResourceInventoryError,
};
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, RuntimeActionRequest, RuntimeHealthState, RuntimeInspection,
    RuntimeMountSource,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

#[path = "real_box_release/fixtures.rs"]
mod fixtures;
#[path = "real_box_release/teardown.rs"]
mod teardown;

use fixtures::*;
use teardown::*;

const AGENT_RUNTIME_IMAGE_ENV: &str = "A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE";
const AGENT_RUNTIME_MEDIA_TYPE_ENV: &str = "A3S_CLOUD_A0_4_AGENT_RUNTIME_MEDIA_TYPE";
const AGENT_RUNTIME_SIZE_ENV: &str = "A3S_CLOUD_A0_4_AGENT_RUNTIME_SIZE_BYTES";
const AGENT_RUNTIME_SCRIPT: &str = "mkdir -p /tmp/a3s-health/health; : > /tmp/a3s-health/health/ready; : > /tmp/a3s-health/health/live; exec httpd -f -p 8080 -h /tmp/a3s-health";
const MAX_MANIFEST_ARCHIVE_BYTES: u64 = 1024 * 1024;

pub(super) async fn exercise(postgres_url: String) -> TestResult {
    require_gate()?;
    let runtime_image = PublishedRuntimeImage::from_environment()?;
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let created_at = canonical_timestamp(Utc::now() - Duration::seconds(10));
    insert_scope(
        &database,
        organization_id,
        project_id,
        environment_id,
        created_at,
    )
    .await?;

    let assets = Arc::new(PostgresAssetRepository::new(executor.clone()));
    let artifacts = Arc::new(HostedArtifactQueryService::new(Arc::new(
        PostgresBuildRunRepository::new(executor.clone()),
    )));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let secrets = Arc::new(PostgresSecretRepository::new(executor.clone()));
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));

    let asset = create_agent_asset(assets.as_ref(), organization_id, created_at).await?;
    let release = create_agent_release(assets.as_ref(), &asset, created_at).await?;
    let publication = crate::build_runs_support::HostedAgentRuntimeArtifact::new(
        runtime_image.artifact.clone(),
        runtime_image.size_bytes,
        release_manifest_template(&runtime_image.artifact.media_type),
    )?;
    let published = crate::build_runs_support::publish_hosted_release_with_runtime_artifact(
        &executor,
        &asset,
        &release,
        &publication,
    )
    .await?;
    verify_publication(&published, &publication)?;

    let manifest = published
        .agent_release_manifest
        .as_ref()
        .ok_or_else(|| invalid("published Agent release omitted its final manifest"))?;
    let manifest_archive = agent_release_manifest_archive(manifest.canonical_acl().as_bytes())?;
    if manifest_archive.len() as u64 != manifest.archive_size_bytes()
        || Sha256Digest::from_bytes(&manifest_archive) != *manifest.archive_digest()
    {
        return Err(invalid("published Agent release manifest archive changed its bytes").into());
    }
    let manifest_artifact = ArtifactRef {
        uri: manifest.archive_uri()?,
        digest: manifest.archive_digest().to_string(),
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    };
    manifest_artifact.validate().map_err(invalid)?;

    let home = dedicated_box_home()?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let proposed_node_id = NodeId::new();
    let transport = Arc::new(PublishedManifestTransport {
        artifact: manifest_artifact.clone(),
        archive: manifest_archive,
        downloads: AtomicUsize::new(0),
    });
    let artifact_manager = Arc::new(
        NodeArtifactManager::new(
            node_state.path(),
            ArtifactConfig {
                max_blob_bytes: MAX_MANIFEST_ARCHIVE_BYTES,
                max_entries: 16,
                max_file_bytes: MAX_MANIFEST_ARCHIVE_BYTES,
                max_expanded_bytes: MAX_MANIFEST_ARCHIVE_BYTES,
            },
            proposed_node_id.as_uuid(),
            transport.clone(),
        )
        .map_err(invalid)?,
    );
    let provider = build_box_runtime_provider(
        &BoxRuntimeConfig {
            home_dir: home.clone(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        runtime_state.path(),
    )?;
    let runtime = provider
        .into_artifact_bound_client(artifact_manager.clone())
        .await?;
    let runtime_capabilities = runtime.capabilities().await?;
    runtime_capabilities.validate()?;
    if !runtime_capabilities
        .artifact_media_types
        .contains(&runtime_image.artifact.media_type)
    {
        return Err(invalid("real Box does not advertise the published Agent media type").into());
    }
    let (node_id, agent_instance_id) = enroll_node(
        nodes.as_ref(),
        organization_id,
        proposed_node_id,
        &runtime_capabilities,
        canonical_timestamp(Utc::now()),
    )
    .await?;
    let inventory = record_inventory(nodes.as_ref(), node_id, agent_instance_id).await?;

    let workload = CreateAgentWorkloadDeploymentHandler::new(
        projects,
        assets.clone(),
        artifacts,
        workloads.clone(),
        secrets,
        nodes.clone(),
    )
    .execute(
        CreateAgentWorkloadDeployment {
            organization_id,
            project_id,
            environment_id,
            asset_id: asset.id,
            asset_release_id: published.id,
            name: "a0-4-real-box-agent".into(),
            node_pool_id: None,
            template: agent_runtime_template(),
            idempotency_key: "deploy-a0-4-real-box-agent".into(),
            request_id: Uuid::now_v7(),
            requested_at: canonical_timestamp(Utc::now())
                .max(published.updated_at + Duration::milliseconds(1)),
        },
        context(),
    )
    .await?
    .map_err(|error| invalid(format!("could not create real Agent Workload: {error}")))?;
    let deployment_id = workload.bundle.deployment.id;
    let workload_id = workload.bundle.workload.id;
    let deployment_operation_id = workload.bundle.operation.id;
    let spec = project_runtime_spec(&workload.bundle.revision)?;
    verify_projected_runtime(
        &spec,
        publication.artifact(),
        &manifest_artifact,
        manifest.identity().as_str(),
    )?;

    let flow_runtime = DeploymentFlowRuntime::new(
        DeploymentFlowDependencies::new(
            workloads.clone(),
            Arc::new(PostgresResourceClaimRepository::new(executor.clone())),
            Arc::new(PinnedArtifactResolver(runtime_image.artifact.clone())),
            nodes.clone(),
            nodes.clone(),
            Arc::new(UnroutedDeploymentRouteUpdater),
        ),
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(
            120_000, 120_000, 25, 120_000, 120_000, 25, 120_000,
        )?,
    )?;
    let flow = FlowInfrastructure::connect(&postgres_url, Arc::new(flow_runtime)).await?;
    let operations: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let coordinator = flow_coordinator(&flow, operations.clone())?;
    let journal = FileCommandJournal::new(node_state.path(), node_id.as_uuid())?;
    let executor_on_node = CommandExecutor::runtime_only(journal, runtime.clone())
        .with_artifacts(artifact_manager)
        .with_resource_inventory(Arc::new(FixedInventory(inventory)));

    let prepare = next_flow_command(
        &coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        0,
        LifecycleCommandKind::ResourcePrepare,
    )
    .await?;
    execute_and_persist(
        &executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        &prepare,
    )
    .await?;

    let apply = next_flow_command(
        &coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        prepare.sequence,
        LifecycleCommandKind::RuntimeApply,
    )
    .await?;
    let NodeCommandPayload::RuntimeApply {
        request,
        resource_claim,
    } = &apply.payload
    else {
        return Err(invalid("deployment Flow emitted a non-apply command").into());
    };
    if request.spec != spec || resource_claim.is_none() {
        return Err(invalid(
            "deployment Flow changed the published Agent Runtime spec or omitted its Claim",
        )
        .into());
    }
    let apply_ack = execute_and_persist(
        &executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        &apply,
    )
    .await?;
    let first_observation = applied_observation(&apply_ack)?.clone();
    verify_running_observation(&first_observation, &spec)?;
    drive_until_active(
        &coordinator,
        workloads.as_ref(),
        operations.as_ref(),
        organization_id,
        deployment_id,
        deployment_operation_id,
    )
    .await?;

    let provider_resource_id = first_observation
        .provider_resource_id
        .clone()
        .ok_or_else(|| invalid("real Agent Runtime observation omitted its Box identity"))?;
    let first_started_at_ms = first_observation
        .started_at_ms
        .ok_or_else(|| invalid("real Agent Runtime observation omitted its start time"))?;
    kill_box_process(&home, &provider_resource_id).await?;
    let mut recovered = wait_for_recovery(
        runtime.as_ref(),
        &spec,
        &provider_resource_id,
        first_started_at_ms,
    )
    .await?;
    if let Some(binding) = resource_claim.as_deref() {
        binding.bind_runtime_observation(&mut recovered)?;
    }
    let recovered_started_at_ms = recovered
        .started_at_ms
        .ok_or_else(|| invalid("recovered Agent Runtime omitted its start time"))?;
    record_recovered_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        recovered.clone(),
    )
    .await?;
    let stored_recovery = nodes
        .latest_runtime_observation(node_id, &spec.unit_id, spec.generation)
        .await?
        .ok_or_else(|| invalid("Fleet did not persist the recovered Agent observation"))?;
    if stored_recovery.observation.provider_resource_id.as_deref()
        != Some(provider_resource_id.as_str())
        || stored_recovery.observation.started_at_ms != Some(recovered_started_at_ms)
    {
        return Err(invalid("Fleet changed the recovered Agent Runtime identity").into());
    }

    let stop_operation_id = request_workload_stop(
        workloads.as_ref(),
        organization_id,
        workload_id,
        canonical_timestamp(Utc::now()),
    )
    .await?;
    let stop = next_flow_command(
        &coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        apply.sequence,
        LifecycleCommandKind::RuntimeStop,
    )
    .await?;
    let stop_ack = execute_and_persist(
        &executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        &stop,
    )
    .await?;
    verify_stopped_acknowledgement(&stop_ack, &spec)?;

    let release_claim = next_flow_command(
        &coordinator,
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        stop.sequence,
        LifecycleCommandKind::ResourceRelease,
    )
    .await?;
    execute_and_persist(
        &executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        &release_claim,
    )
    .await?;
    drive_until_stopped(
        &coordinator,
        workloads.as_ref(),
        operations.as_ref(),
        organization_id,
        workload_id,
        stop_operation_id,
    )
    .await?;

    let remove = enqueue_remove(
        nodes.as_ref(),
        node_id,
        workload_id.as_uuid(),
        stop_operation_id.as_uuid(),
        &spec,
    )
    .await?;
    let leased_remove = lease_only_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        release_claim.sequence,
    )
    .await?;
    if leased_remove.command_id != remove.id.as_uuid()
        || !matches!(
            leased_remove.payload,
            NodeCommandPayload::RuntimeRemove { .. }
        )
    {
        return Err(invalid("Fleet changed the persisted Agent Runtime removal").into());
    }
    let remove_ack = execute_and_persist(
        &executor_on_node,
        &nodes,
        node_id,
        agent_instance_id,
        &runtime_capabilities,
        &leased_remove,
    )
    .await?;
    verify_removed_acknowledgement(&remove_ack, &spec)?;
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("removed Agent Runtime remained inspectable").into());
    }
    verify_clean_state(&home, node_state.path())?;

    if transport.downloads.load(Ordering::SeqCst) != 1 {
        return Err(invalid("published Agent manifest was not downloaded exactly once").into());
    }
    let (command_count, acknowledgement_count) = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select count(*), count(acknowledgement) from node_commands where node_id = ",
            )
            .bind(node_id.as_uuid()),
        )
        .await?;
    let observation_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from runtime_observations where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and unit_id = ")
                .bind(spec.unit_id.clone())
                .append(" and generation = ")
                .bind(spec.generation),
        )
        .await?;
    if (command_count, acknowledgement_count, observation_count) != (5, 5, 3) {
        return Err(invalid(format!(
            "unexpected durable Agent lifecycle evidence: commands={command_count}, acknowledgements={acknowledgement_count}, observations={observation_count}"
        ))
        .into());
    }

    println!(
        "A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED store=postgresql release=published deployment_flow=completed stop_flow=completed runtime_apply=persisted provider=a3s-box observations=3 process_restarts=1 artifact_downloads=1 commands=5 acknowledgements=5 cleanup=removed artifact_digest={} manifest_identity={} first_started_at_ms={} recovered_started_at_ms={}",
        spec.artifact.digest,
        manifest.identity(),
        first_started_at_ms,
        recovered_started_at_ms
    );
    Ok(())
}

async fn create_agent_asset(
    assets: &PostgresAssetRepository,
    organization_id: OrganizationId,
    created_at: DateTime<Utc>,
) -> TestResult<Asset> {
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("A0.4 Real Box Agent")?,
        AssetKind::Agent,
        created_at,
    )?;
    assets
        .create_asset(CreateAssetWrite {
            asset: asset.clone(),
            event: AssetCreated::envelope(&asset, Uuid::now_v7())?,
            idempotency: idempotency(
                &format!("test.a0-4.organizations/{organization_id}/assets"),
                "create-real-box-agent",
                b"create-real-box-agent",
            )?,
        })
        .await?;
    Ok(asset)
}

async fn create_agent_release(
    assets: &PostgresAssetRepository,
    asset: &Asset,
    created_at: DateTime<Utc>,
) -> TestResult<AssetRelease> {
    let release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0")?,
        GitCommitSha::parse("a".repeat(40))?,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))?,
        created_at + Duration::milliseconds(1),
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            release: release.clone(),
            event: AssetReleaseDrafted::envelope(&release, release.id.as_uuid())?,
            hosted_build_requested_event: Some(HostedAssetBuildRequested::envelope(
                asset,
                &release,
                release.id.as_uuid(),
            )?),
            idempotency: idempotency(
                &format!("test.a0-4.organizations/{}/releases", asset.organization_id),
                "draft-real-box-agent-1.0.0",
                b"draft-real-box-agent-1.0.0",
            )?,
        })
        .await?;
    Ok(release)
}

fn verify_publication(
    release: &AssetRelease,
    expected: &crate::build_runs_support::HostedAgentRuntimeArtifact,
) -> TestResult {
    let artifact = release
        .artifact
        .as_ref()
        .ok_or_else(|| invalid("published Agent release omitted its OCI artifact"))?;
    if artifact.digest().as_str() != expected.artifact().digest
        || artifact.media_type() != expected.artifact().media_type
        || artifact.size_bytes() != expected.size_bytes()
    {
        return Err(invalid("published Agent release changed its exact OCI artifact").into());
    }
    Ok(())
}

fn release_manifest_template(media_type: &str) -> String {
    format!(
        concat!(
            "agent_release {{\n",
            "  schema = \"a3s.code.agent-release.v1\"\n",
            "  protocol = \"a3s.code.agent.v1\"\n",
            "  artifact {{ digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"{}\" }}\n",
            "  entrypoint {{ command = \"/bin/sh\" args = [\"-c\", \"{}\"] }}\n",
            "  health {{ transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }}\n",
            "  storage {{ workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }}\n",
            "  capability \"runtime.service\" {{ level = 1 }}\n",
            "  capability \"secrets.external\" {{ level = 1 }}\n",
            "  capability \"workspace.local\" {{ level = 1 }}\n",
            "  provenance \"source\" {{ uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }}\n",
            "  provenance \"builder\" {{ uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }}\n",
            "}}\n"
        ),
        media_type, AGENT_RUNTIME_SCRIPT
    )
}

fn verify_projected_runtime(
    spec: &RuntimeUnitSpec,
    image: &ArtifactRef,
    manifest_artifact: &ArtifactRef,
    manifest_identity: &str,
) -> TestResult {
    if &spec.artifact != image
        || spec.process.command != ["/bin/sh"]
        || spec.process.args != ["-c", AGENT_RUNTIME_SCRIPT]
        || spec.generation != 1
    {
        return Err(invalid("Agent Workload changed its release-owned Runtime intent").into());
    }
    let Some(health) = &spec.health else {
        return Err(invalid("Agent Workload omitted its readiness probe").into());
    };
    let Some(lifecycle) = &spec.service_lifecycle else {
        return Err(invalid("Agent Workload omitted its liveness policy").into());
    };
    if !matches!(
        &health.probe,
        HealthProbe::Http { path, .. } if path == "/health/ready"
    ) || !matches!(
        &lifecycle.liveness.probe,
        HealthProbe::Http { path, .. } if path == "/health/live"
    ) {
        return Err(invalid("Agent Workload changed its manifest-owned health policy").into());
    }
    let manifest_mount = spec
        .mounts
        .iter()
        .find(|mount| mount.name == "agent-release-manifest")
        .ok_or_else(|| invalid("Agent Workload omitted its release manifest mount"))?;
    if manifest_mount.target != "/app/.a3s"
        || !manifest_mount.read_only
        || !matches!(
            &manifest_mount.source,
            RuntimeMountSource::Artifact { artifact } if artifact == manifest_artifact
        )
    {
        return Err(invalid("Agent Workload changed its exact manifest Artifact mount").into());
    }
    Sha256Digest::parse(manifest_identity)?;
    Ok(())
}

fn verify_running_observation(
    observation: &RuntimeObservation,
    spec: &RuntimeUnitSpec,
) -> TestResult {
    observation.validate_against(spec)?;
    if observation.state != RuntimeUnitState::Running
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
        || observation
            .liveness
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
    {
        return Err(invalid("published Agent release did not become healthy in real Box").into());
    }
    Ok(())
}

async fn record_inventory(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
) -> TestResult<NodeResourceInventory> {
    let inventory = NodeResourceInventory::new(
        node_id.as_uuid(),
        agent_instance_id,
        1,
        canonical_timestamp(Utc::now()),
        vec![
            NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/shared",
                ResourceAllocation::Scalar {
                    amount: 8_000,
                    unit: ResourceUnit::MilliCpu,
                },
            )?,
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/system",
                ResourceAllocation::Scalar {
                    amount: 8 * 1024 * 1024 * 1024,
                    unit: ResourceUnit::Byte,
                },
            )?,
        ],
    )?;
    nodes
        .record_resource_inventory(inventory.clone(), canonical_timestamp(Utc::now()))
        .await?;
    Ok(inventory)
}

fn flow_coordinator(
    flow: &FlowInfrastructure,
    operations: Arc<dyn IOperationRepository>,
) -> TestResult<FlowOperationCoordinator> {
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operations,
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        100,
    );
    Ok(FlowOperationCoordinator::new(
        reconciler,
        flow,
        StdDuration::from_millis(5),
        StdDuration::from_secs(1),
    )?)
}

#[derive(Debug, Clone, Copy)]
enum LifecycleCommandKind {
    ResourcePrepare,
    RuntimeApply,
    RuntimeStop,
    ResourceRelease,
}

impl LifecycleCommandKind {
    fn matches(self, payload: &NodeCommandPayload) -> bool {
        matches!(
            (self, payload),
            (
                Self::ResourcePrepare,
                NodeCommandPayload::ResourceClaimPrepare { .. }
            ) | (Self::RuntimeApply, NodeCommandPayload::RuntimeApply { .. })
                | (Self::RuntimeStop, NodeCommandPayload::RuntimeStop { .. })
                | (
                    Self::ResourceRelease,
                    NodeCommandPayload::ResourceClaimRelease { .. }
                )
        )
    }
}

async fn next_flow_command(
    coordinator: &FlowOperationCoordinator,
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
    expected: LifecycleCommandKind,
) -> TestResult<NodeCommandEnvelope> {
    let deadline = Instant::now() + StdDuration::from_secs(60);
    loop {
        coordinator.run_once().await?;
        let lease = nodes
            .lease_commands(
                &NodeCommandLeaseRequest {
                    schema: NodeCommandLeaseRequest::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    after_sequence,
                    max_commands: 1,
                    wait_ms: 0,
                },
                Uuid::now_v7(),
                canonical_timestamp(Utc::now()),
                canonical_timestamp(Utc::now() + Duration::seconds(10)),
            )
            .await?;
        if let Some(command) = lease.commands.into_iter().next() {
            if !expected.matches(&command.payload) {
                return Err(invalid(format!(
                    "deployment Flow emitted the wrong command while waiting for {expected:?}"
                ))
                .into());
            }
            return Ok(command);
        }
        if Instant::now() >= deadline {
            return Err(invalid(format!(
                "deployment Flow did not emit {expected:?} within 60 seconds"
            ))
            .into());
        }
        tokio::time::sleep(StdDuration::from_millis(10)).await;
    }
}

async fn execute_and_persist(
    executor: &CommandExecutor,
    nodes: &Arc<PostgresNodeRepository>,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: &RuntimeCapabilities,
    command: &NodeCommandEnvelope,
) -> TestResult<NodeCommandAck> {
    let acknowledgement = executor.execute(command.clone()).await?;
    crate::deployment_flow_support::persist_command_result(
        nodes,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        acknowledgement.clone(),
    )
    .await?;
    Ok(acknowledgement)
}

fn applied_observation(acknowledgement: &NodeCommandAck) -> TestResult<&RuntimeObservation> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
            result => Err(invalid(format!("real Agent apply returned {result:?}")).into()),
        },
        outcome => Err(invalid(format!("real Agent apply failed: {outcome:?}")).into()),
    }
}

async fn drive_until_active(
    coordinator: &FlowOperationCoordinator,
    workloads: &PostgresWorkloadRepository,
    operations: &dyn IOperationRepository,
    organization_id: OrganizationId,
    deployment_id: a3s_cloud_control_plane::modules::shared_kernel::domain::DeploymentId,
    operation_id: OperationId,
) -> TestResult {
    let deadline = Instant::now() + StdDuration::from_secs(60);
    loop {
        coordinator.run_once().await?;
        let deployment = workloads
            .find_deployment(organization_id, deployment_id)
            .await?;
        let operation = operations.find_projection(operation_id).await?;
        if deployment.status == DeploymentStatus::Active
            && operation.is_some_and(|projection| projection.status == OperationStatus::Succeeded)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(invalid(format!(
                "published Agent deployment did not become active: {}",
                deployment.status.as_str()
            ))
            .into());
        }
        tokio::time::sleep(StdDuration::from_millis(10)).await;
    }
}

async fn kill_box_process(home: &Path, provider_resource_id: &str) -> TestResult {
    let store = BoxStateStore::load_readonly(home.join("boxes.json"))?;
    let records = store
        .records()
        .iter()
        .filter(|record| record.id == provider_resource_id)
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err(invalid(format!(
            "Box state contained {} published Agent records",
            records.len()
        ))
        .into());
    }
    let pid = records[0]
        .pid
        .ok_or_else(|| invalid("published Agent Box record omitted its process PID"))?;
    if pid <= 1 || pid == std::process::id() {
        return Err(invalid(format!("published Agent exposed unsafe PID {pid}")).into());
    }
    let status = tokio::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .await?;
    if !status.success() {
        return Err(invalid(format!("could not SIGKILL published Agent PID {pid}")).into());
    }
    Ok(())
}

async fn wait_for_recovery(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    provider_resource_id: &str,
    first_started_at_ms: u64,
) -> TestResult<RuntimeObservation> {
    let deadline = Instant::now() + StdDuration::from_secs(30);
    loop {
        match runtime.inspect(&spec.unit_id).await? {
            RuntimeInspection::Found { observation, .. }
                if observation.state == RuntimeUnitState::Running
                    && observation.provider_resource_id.as_deref()
                        == Some(provider_resource_id)
                    && observation
                        .started_at_ms
                        .is_some_and(|started_at_ms| started_at_ms > first_started_at_ms)
                    && observation
                        .health
                        .as_ref()
                        .is_some_and(|health| health.state == RuntimeHealthState::Healthy)
                    && observation
                        .liveness
                        .as_ref()
                        .is_some_and(|health| health.state == RuntimeHealthState::Healthy) =>
            {
                return Ok(observation.as_ref().clone());
            }
            RuntimeInspection::NotFound { .. } => {
                return Err(invalid("Box lost the published Agent Runtime identity").into())
            }
            RuntimeInspection::Found { .. } => {}
        }
        if Instant::now() >= deadline {
            return Err(
                invalid("Box did not recover the published Agent within 30 seconds").into(),
            );
        }
        tokio::time::sleep(StdDuration::from_millis(25)).await;
    }
}

async fn record_recovered_observation(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: &RuntimeCapabilities,
    observation: RuntimeObservation,
) -> TestResult {
    let observed_at = canonical_timestamp(Utc::now());
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at,
                    agent_version: "0.1.0-test".into(),
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: None,
                    observed_at,
                    observation,
                }],
            }
            .into(),
            observed_at,
        )
        .await?;
    Ok(())
}
