use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandResult, NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::EnrollmentToken;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnrollmentTokenId, EnvironmentId, IdempotencyRequest, NodeId, OperationId,
    OrganizationId, ProjectId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentFlowConfig, DeploymentFlowRuntime,
    DeploymentRequested, DeploymentStatus, HttpHealthCheck, IOciArtifactResolver,
    IWorkloadRepository, InMemoryWorkloadRepository, OciArtifact, OciArtifactReference,
    OciArtifactResolutionError, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    Workload, WorkloadRevision,
};
use a3s_cloud_node_agent::{
    CommandExecutor, DockerConfig, DockerRuntimeDriver, FileCommandJournal, NodeRuntimeBinding,
};
use a3s_flow::{FlowEngine, WorkflowRunStatus, WorkflowSpec};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeHealthState, RuntimeInspection, RuntimeUnitState,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeDriver, RuntimeStateStore,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

const BUSYBOX_DIGEST: &str =
    "sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662";

#[tokio::test]
async fn permanently_unhealthy_real_docker_update_preserves_healthy_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref() != Ok("1") {
        return Ok(());
    }

    let state_directory = tempfile::tempdir()?;
    let namespace = format!(
        "cloud-update-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    );
    let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
        socket: docker_socket(),
        namespace,
        operation_timeout_ms: 30_000,
        secret_memory_dir: "/dev/shm/a3s-cloud/test-secrets".into(),
    })?);
    let capabilities = RuntimeDriver::capabilities(driver.as_ref()).await?;

    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        ready_node(&nodes, organization_id, base, capabilities.clone()).await?;
    driver.bind_node(node_id.as_uuid()).await?;

    let workload_port: Arc<dyn IWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let flow_runtime = DeploymentFlowRuntime::new(
        workload_port,
        Arc::new(UnusedArtifactResolver),
        node_port,
        control_port,
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(30_000, 20_000, 5, 30_000, 10_000, 5, 20_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(flow_runtime));

    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("real Docker failed update")?,
        base,
    );
    let first = deployment_bundle(
        workload,
        1,
        healthy_template(),
        base,
        "docker-healthy-first",
    )?;
    let first_revision = first.revision.clone();
    let first_deployment = first.deployment.clone();
    let first_operation = first.operation.clone();
    workloads.create_deployment(first).await?;
    engine
        .start_with_id(
            first_operation.id.to_string(),
            workflow_spec(),
            first_operation.input,
        )
        .await?;

    let runtime_state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
        state_directory.path().join("runtime"),
    ));
    let runtime_driver: Arc<dyn RuntimeDriver> = driver.clone();
    let runtime_client: Arc<dyn RuntimeClient> = Arc::new(ManagedRuntimeClient::new(
        runtime_state.clone(),
        runtime_driver,
    ));
    let command_executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(state_directory.path().join("journal"), node_id.as_uuid())?,
        runtime_client,
    );

    let first_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(first_lease.commands.len(), 1);
    let first_ack = command_executor
        .execute(first_lease.commands[0].clone())
        .await?;
    let first_observation = persist_apply_result(
        &nodes,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        first_ack,
    )
    .await?;
    assert_eq!(
        first_observation.health.as_ref().map(|health| health.state),
        Some(RuntimeHealthState::Healthy)
    );
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        workloads
            .find_deployment(organization_id, first_deployment.id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    let first_record = runtime_state.load(&first_observation.unit_id).await?;

    let active_workload = workloads
        .find_workload(organization_id, first_deployment.workload_id)
        .await?;
    let second = deployment_bundle(
        active_workload,
        2,
        unhealthy_template(),
        Utc::now(),
        "docker-unhealthy-update",
    )?;
    let second_revision = second.revision.clone();
    let second_deployment = second.deployment.clone();
    let second_operation = second.operation.clone();
    workloads.create_deployment(second).await?;
    engine
        .start_with_id(
            second_operation.id.to_string(),
            workflow_spec(),
            second_operation.input,
        )
        .await?;

    let second_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        first_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(second_lease.commands.len(), 1);
    let second_ack = command_executor
        .execute(second_lease.commands[0].clone())
        .await?;
    let second_observation =
        persist_apply_result(&nodes, node_id, agent_instance_id, capabilities, second_ack).await?;
    assert_eq!(second_observation.state, RuntimeUnitState::Running);
    assert_eq!(
        second_observation
            .health
            .as_ref()
            .map(|health| health.state),
        Some(RuntimeHealthState::Unhealthy)
    );
    assert_ne!(
        first_observation.provider_resource_id,
        second_observation.provider_resource_id
    );

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine
            .snapshot(&second_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, second_deployment.id)
            .await?
            .status,
        DeploymentStatus::Failed
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, first_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    assert_ne!(first_revision.id, second_revision.id);

    match RuntimeDriver::inspect(driver.as_ref(), &first_record).await? {
        RuntimeInspection::Found { observation, .. } => {
            assert_eq!(observation.state, RuntimeUnitState::Running);
            assert_eq!(
                observation.health.as_ref().map(|health| health.state),
                Some(RuntimeHealthState::Healthy)
            );
        }
        RuntimeInspection::NotFound { .. } => {
            return Err("the prior healthy Docker container disappeared".into())
        }
    }

    let second_record = runtime_state.load(&second_observation.unit_id).await?;
    remove_record(driver.as_ref(), &second_record, "unhealthy").await?;
    remove_record(driver.as_ref(), &first_record, "healthy").await?;
    Ok(())
}

async fn ready_node(
    nodes: &Arc<InMemoryNodeRepository>,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
    capabilities: a3s_runtime::contract::RuntimeCapabilities,
) -> Result<(NodeId, Uuid), Box<dyn std::error::Error>> {
    let secret = format!("a3sn_{}", "d".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "docker-update-test",
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token.clone(),
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "fleet.enrollment-token.issued".into(),
                schema_version: 1,
                organization_id: organization_id.as_uuid(),
                aggregate_id: token.id.as_uuid(),
                aggregate_version: token.aggregate_version,
                occurred_at: enrolled_at,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: serde_json::json!({"name": "docker-update-test"}),
            },
            IdempotencyRequest::new("test.enrollment", "docker-update-test", b"token")?,
        )
        .await?;
    let stored_capabilities = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(&capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("docker-update-node")?,
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: stored_capabilities.clone(),
                request_digest: format!("sha256:{}", "e".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored_capabilities,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

fn deployment_bundle(
    workload: Workload,
    generation: u64,
    template: ServiceTemplate,
    requested_at: chrono::DateTime<Utc>,
    idempotency_key: &str,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        generation,
        template,
        requested_at,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        requested_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        workload.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "1")?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": workload.organization_id,
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    Ok(CreateDeploymentBundle {
        workload,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new(
            "test.workload.deploy",
            idempotency_key,
            idempotency_key.as_bytes(),
        )?,
        event,
    })
}

async fn lease(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> Result<a3s_cloud_contracts::NodeCommandLeaseResponse, Box<dyn std::error::Error>> {
    let now = Utc::now();
    Ok(nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(10),
        )
        .await?)
}

async fn persist_apply_result(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: a3s_runtime::contract::RuntimeCapabilities,
    acknowledgement: NodeCommandAck,
) -> Result<a3s_runtime::contract::RuntimeObservation, Box<dyn std::error::Error>> {
    let observation = match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeApplied { observation } => observation.as_ref().clone(),
            _ => return Err("Docker command returned a non-apply result".into()),
        },
        outcome => return Err(format!("Docker apply failed: {outcome:?}").into()),
    };
    let completed_at = acknowledgement.completed_at;
    let sent_at = Utc::now().max(completed_at);
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at: sent_at,
                    agent_version: "0.1.0".into(),
                    runtime_capabilities: capabilities,
                },
                observations: vec![RuntimeObservationReport {
                    report_id: acknowledgement.command_id,
                    command_id: Some(acknowledgement.command_id),
                    observed_at: completed_at,
                    observation: observation.clone(),
                }],
            },
            sent_at,
        )
        .await?;
    assert!(
        !nodes
            .acknowledge_command(acknowledgement, sent_at)
            .await?
            .replayed
    );
    Ok(observation)
}

async fn remove_record(
    driver: &DockerRuntimeDriver,
    record: &a3s_runtime::RuntimeUnitRecord,
    suffix: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    RuntimeDriver::remove(
        driver,
        record,
        &RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("docker-update-cleanup-{suffix}-{}", Uuid::now_v7()),
            unit_id: record.spec.unit_id.clone(),
            generation: record.spec.generation,
            deadline_at_ms: None,
        },
    )
    .await?;
    Ok(())
}

fn healthy_template() -> ServiceTemplate {
    busybox_template(
        vec!["/bin/sh".into()],
        vec![
            "-c".into(),
            "mkdir -p /www && printf 'healthy\\n' >/www/index.html && exec httpd -f -p 8080 -h /www"
                .into(),
        ],
    )
}

fn unhealthy_template() -> ServiceTemplate {
    busybox_template(
        vec!["/bin/sh".into()],
        vec!["-c".into(), "exec sleep 300".into()],
    )
}

fn busybox_template(command: Vec<String>, args: Vec<String>) -> ServiceTemplate {
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://docker.io/library/busybox@{BUSYBOX_DIGEST}"),
            digest: BUSYBOX_DIGEST.into(),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command,
            args,
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: HttpHealthCheck {
            port_name: "http".into(),
            path: "/".into(),
            interval_ms: 50,
            timeout_ms: 50,
            healthy_threshold: 2,
            unhealthy_threshold: 2,
            stabilization_window_ms: 50,
        },
    }
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cloud.deployment", "1", "a3s-cloud", "main")
}

fn docker_socket() -> String {
    std::env::var("A3S_CLOUD_TEST_DOCKER_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".into())
}

struct UnusedArtifactResolver;

#[async_trait]
impl IOciArtifactResolver for UnusedArtifactResolver {
    async fn resolve(
        &self,
        _reference: &OciArtifactReference,
        _registry_credential: Option<
            &a3s_cloud_control_plane::modules::workloads::OciRegistryCredentialReference,
        >,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        Err(OciArtifactResolutionError::Registry(
            "resolved Docker fixture unexpectedly called the OCI resolver".into(),
        ))
    }
}
