use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    CloudSecretReference, DomainEventEnvelope, NodeCommandAck, NodeCommandLeaseRequest,
    NodeCommandOutcome, NodeCommandResult, NodeHeartbeat, NodeLogChunkBatch, NodeLogChunkReport,
    NodeObservationBatch, RuntimeObservationReport, RuntimeServiceEndpoint,
};
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::fleet::domain::entities::EnrollmentToken;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::services::ILogChunkStore;
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::{
    LocalKeyEncryptionService, LocalLogChunkStore, PostgresNodeRepository, RecordNodeLogChunks,
    RecordNodeLogChunksHandler,
};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, OperationStatus,
    PostgresOperationRepository, ReconcileOperationsHandler,
};
use a3s_cloud_control_plane::modules::secrets::{
    ISecretEncryptionService, ISecretRepository, PostgresSecretRepository, ResolveSecretMaterial,
    ResolveSecretMaterialHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnrollmentTokenId, IdempotencyRequest, NodeId, OperationId, OrganizationId,
};
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentCancellationRequested, DeploymentFlowConfig, DeploymentFlowRuntime, DeploymentStatus,
    IOciArtifactResolver, IWorkloadRepository, IWorkloadRuntimeControl,
    IWorkloadRuntimeTargetRepository, OciArtifact, OciArtifactReference,
    OciArtifactResolutionError, OciRegistryArtifactResolver, PostgresWorkloadRepository,
    RequestDeploymentCancellationBundle, WorkloadRuntimeReconciler,
};
use a3s_cloud_node_agent::{
    CommandExecutor, DockerConfig, DockerRuntimeDriver, FileCommandJournal, NodeControlClientError,
    NodeRuntimeBinding, NodeSecretTransport, SecretMaterial,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeActionRequest,
    RuntimeCapabilities, RuntimeEvidence, RuntimeFeature, RuntimeHealthObservation,
    RuntimeHealthState, RuntimeInspection, RuntimeLogQuery, RuntimeLogStream, RuntimeObservation,
    RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeDriver, RuntimeStateStore,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub async fn exercise_deployment_flow(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
    security_state_dir: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "update nodes set state = 'draining', aggregate_version = aggregate_version + 1 where organization_id = ",
            )
            .bind(organization_uuid)
            .append(" and state = 'ready'"),
        )
        .await?;
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&node_repository, organization_id).await?;
    let workloads: Arc<dyn IWorkloadRepository> = workload_repository.clone();
    let nodes: Arc<dyn INodeRepository> = node_repository.clone();
    let node_control: Arc<dyn INodeControlRepository> = node_repository.clone();
    let runtime = DeploymentFlowRuntime::new(
        workloads,
        deployment_artifact_resolver(executor, security_state_dir)?,
        nodes,
        node_control,
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_id = OperationId::from_uuid(field_uuid(response, "operationId")?);
    let deployment_id = DeploymentId::from_uuid(field_uuid(response, "deploymentId")?);
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;

    let mut reconciled_before_apply = 0;
    for _ in 0..8 {
        let cycle = coordinator.run_once().await?;
        reconciled_before_apply += cycle.reconciled_before_work;
        if workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status
            == DeploymentStatus::Applying
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(reconciled_before_apply > 0);
    let applying = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(applying.status, DeploymentStatus::Applying);
    let command_id = applying.command_id.ok_or("deployment has no command")?;
    let now = Utc::now();
    let lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: 0,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + ChronoDuration::seconds(10),
        )
        .await?;
    let command = lease
        .commands
        .iter()
        .find(|command| command.command_id == command_id.as_uuid())
        .ok_or("deployment command was not leased")?;
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err("deployment command is not Runtime apply".into());
    };
    let mut docker_runtime: Option<Arc<dyn RuntimeClient>> = None;
    let mut docker_state_directory = None;
    let mut docker_secret_directory = None;
    let (observation, acknowledgement, observed_at) = if docker_tests_enabled() {
        let state_directory = tempfile::tempdir()?;
        let namespace = format!("cloud-flow-{}", &Uuid::now_v7().simple().to_string()[..12]);
        let secret_memory_dir = docker_secret_memory_dir();
        let secret_namespace_dir = secret_memory_dir.join(&namespace);
        let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
            socket: docker_socket(),
            namespace: namespace.clone(),
            operation_timeout_ms: 30_000,
            secret_memory_dir: secret_memory_dir.clone(),
        })?);
        driver.bind_node(node_id.as_uuid()).await?;
        let secret_workloads: Arc<dyn IWorkloadRepository> = workload_repository.clone();
        let secret_transport: Arc<dyn NodeSecretTransport> =
            Arc::new(PostgresSecretTransport::new(
                executor,
                secret_workloads,
                organization_id,
                node_id,
                security_state_dir,
            )?);
        driver.bind_secret_transport(secret_transport).await?;
        let state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
            state_directory.path().join("runtime"),
        ));
        let runtime_driver: Arc<dyn RuntimeDriver> = driver;
        let runtime: Arc<dyn RuntimeClient> =
            Arc::new(ManagedRuntimeClient::new(state, runtime_driver));
        let command_executor = CommandExecutor::runtime_only(
            FileCommandJournal::new(state_directory.path().join("journal"), node_id.as_uuid())?,
            runtime.clone(),
        );
        let serialized_command = serde_json::to_string(command)?;
        assert!(sensitive_plaintexts
            .iter()
            .all(|plaintext| !serialized_command.contains(plaintext)));
        let acknowledgement = command_executor.execute(command.clone()).await?;
        assert_secret_file_modes(&secret_namespace_dir, &[0o400])?;
        assert_eq!(
            command_executor.execute(command.clone()).await?,
            acknowledgement
        );
        assert_tree_excludes_plaintext(state_directory.path(), sensitive_plaintexts)?;
        let observation = match &acknowledgement.outcome {
            a3s_cloud_contracts::NodeCommandOutcome::Succeeded { result } => {
                match result.as_ref() {
                    a3s_cloud_contracts::NodeCommandResult::RuntimeApplied { observation } => {
                        observation.as_ref().clone()
                    }
                    _ => return Err("Docker command returned the wrong result kind".into()),
                }
            }
            outcome => return Err(format!("Docker Runtime apply failed: {outcome:?}").into()),
        };
        persist_redacted_docker_logs(
            executor,
            node_id,
            Arc::clone(&runtime),
            &request.spec,
            security_state_dir,
            sensitive_plaintexts,
        )
        .await?;
        let observed_at = acknowledgement.completed_at;
        docker_runtime = Some(runtime);
        docker_state_directory = Some(state_directory);
        docker_secret_directory = Some(secret_namespace_dir);
        (observation, Some(acknowledgement), observed_at)
    } else {
        (healthy_observation(&request.spec)?, None, Utc::now())
    };
    let sent_at = Utc::now().max(observed_at);
    node_repository
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
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: command.command_id,
                    command_id: Some(command.command_id),
                    observed_at,
                    observation,
                }],
            },
            observed_at,
        )
        .await?;
    if let Some(acknowledgement) = acknowledgement {
        assert!(
            !node_repository
                .acknowledge_command(acknowledgement, sent_at)
                .await?
                .replayed
        );
    }
    let before_restart = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(before_restart.status, DeploymentStatus::Applying);
    assert!(workload_repository
        .find_workload(organization_id, before_restart.workload_id)
        .await?
        .active_revision_id
        .is_none());
    assert!(INodeControlRepository::latest_runtime_observation(
        node_repository.as_ref(),
        node_id,
        &request.spec.unit_id,
        request.spec.generation,
    )
    .await?
    .is_some());

    // Simulate control-plane loss after health evidence is durable but before
    // the deployment verification and activation projections are written.
    drop(coordinator);
    drop(flow);
    let restarted_runtime = DeploymentFlowRuntime::new(
        workload_repository.clone(),
        deployment_artifact_resolver(executor, security_state_dir)?,
        node_repository.clone(),
        node_repository.clone(),
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(restarted_runtime)).await?;
    let restarted_reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        restarted_reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;
    let mut handled_after_restart = 0;
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let cycle = coordinator.run_once().await?;
        handled_after_restart += cycle.handled_tasks;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        let operation = operation_repository.find_projection(operation_id).await?;
        if deployment.status == DeploymentStatus::Active
            && operation.is_some_and(|projection| projection.status == OperationStatus::Succeeded)
        {
            break;
        }
    }
    assert!(handled_after_restart > 0);
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("deployment operation has no projection")?
            .status,
        OperationStatus::Succeeded
    );
    assert_eq!(
        workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status,
        DeploymentStatus::Active
    );

    let target_port: Arc<dyn IWorkloadRuntimeTargetRepository> = workload_repository.clone();
    let runtime_control: Arc<dyn IWorkloadRuntimeControl> = node_repository.clone();
    let workload_reconciler = WorkloadRuntimeReconciler::new(
        target_port,
        runtime_control,
        Duration::from_millis(1),
        Duration::from_secs(10),
        Duration::from_secs(5),
        100,
    )?;
    tokio::time::sleep(Duration::from_millis(2)).await;
    let inspection_cycle = workload_reconciler.run_once(Utc::now()).await?;
    assert_eq!(inspection_cycle.inspect_commands, 1);
    let inspect_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: command.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let inspect_command = inspect_lease
        .commands
        .first()
        .ok_or("workload reconciliation did not dispatch Runtime inspect")?;
    assert!(matches!(
        inspect_command.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeInspect { .. }
    ));
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: inspect_command.command_id,
            lease_id: inspect_command.lease_id,
            node_id: inspect_command.node_id,
            sequence: inspect_command.sequence,
            payload_digest: inspect_command.payload_digest.clone(),
            completed_at: Utc::now(),
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeInspected {
                    inspection: RuntimeInspection::NotFound {
                        schema: RuntimeInspection::SCHEMA.into(),
                        unit_id: request.spec.unit_id.clone(),
                        last_generation: Some(request.spec.generation),
                    },
                }),
            },
        },
    )
    .await?;

    let recovery_cycle = workload_reconciler.run_once(Utc::now()).await?;
    assert_eq!(
        recovery_cycle.recovery_commands, 1,
        "unexpected recovery cycle: {recovery_cycle:?}"
    );
    let pending_replay = workload_reconciler.run_once(Utc::now()).await?;
    assert_eq!(pending_replay.recovery_commands, 0);
    assert_eq!(pending_replay.pending_commands, 1);
    let recovery_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: inspect_command.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let recovery_command = recovery_lease
        .commands
        .first()
        .ok_or("workload reconciliation did not dispatch Runtime recovery")?;
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply {
        request: recovery_request,
    } = &recovery_command.payload
    else {
        return Err("workload reconciliation recovery is not Runtime apply".into());
    };
    assert_eq!(recovery_request.spec.generation, request.spec.generation);
    assert_eq!(recovery_request.spec.digest()?, request.spec.digest()?);
    assert_eq!(
        recovery_request.spec.artifact.digest,
        request.spec.artifact.digest
    );
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: recovery_command.command_id,
            lease_id: recovery_command.lease_id,
            node_id: recovery_command.node_id,
            sequence: recovery_command.sequence,
            payload_digest: recovery_command.payload_digest.clone(),
            completed_at: Utc::now(),
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeApplied {
                    observation: Box::new(healthy_observation(&request.spec)?),
                }),
            },
        },
    )
    .await?;
    assert!(workload_reconciler
        .run_once(Utc::now())
        .await?
        .failures
        .is_empty());

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where id = ")
                    .bind(command_id.as_uuid()),
            )
            .await?,
        1
    );
    let history_length = flow
        .engine()
        .history(&operation_id.to_string())
        .await?
        .len();
    coordinator.run_once().await?;
    assert_eq!(
        flow.engine()
            .history(&operation_id.to_string())
            .await?
            .len(),
        history_length
    );
    if let Some(runtime) = docker_runtime {
        runtime
            .remove(&RuntimeActionRequest {
                schema: RuntimeActionRequest::SCHEMA.into(),
                request_id: format!("integration-cleanup-{}", Uuid::now_v7()),
                unit_id: request.spec.unit_id.clone(),
                generation: request.spec.generation,
                deadline_at_ms: None,
            })
            .await?;
    }
    if let Some(directory) = docker_secret_directory {
        match tokio::fs::remove_dir(directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    drop(docker_state_directory);
    Ok(())
}

struct PostgresSecretTransport {
    handler: ResolveSecretMaterialHandler,
    organization_id: OrganizationId,
    node_id: NodeId,
}

impl PostgresSecretTransport {
    fn new(
        executor: &PostgresExecutor,
        workloads: Arc<dyn IWorkloadRepository>,
        organization_id: OrganizationId,
        node_id: NodeId,
        security_state_dir: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let secrets: Arc<dyn ISecretRepository> =
            Arc::new(PostgresSecretRepository::new(executor.clone()));
        let encryption: Arc<dyn ISecretEncryptionService> =
            Arc::new(LocalKeyEncryptionService::load_or_create(
                security_state_dir.join("key-encryption.key"),
            )?);
        Ok(Self {
            handler: ResolveSecretMaterialHandler::new(workloads, secrets, encryption),
            organization_id,
            node_id,
        })
    }
}

#[async_trait]
impl NodeSecretTransport for PostgresSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        let plaintext = self
            .handler
            .execute(
                ResolveSecretMaterial {
                    organization_id: self.organization_id,
                    authenticated_node_id: self.node_id,
                    reference,
                },
                context(),
            )
            .await
            .map_err(|_| {
                NodeControlClientError::Transport("PostgreSQL Secret material query failed".into())
            })?
            .map_err(secret_application_error)?;
        SecretMaterial::new(plaintext.as_bytes().to_vec()).map_err(NodeControlClientError::Invalid)
    }
}

fn secret_application_error(error: ApplicationError) -> NodeControlClientError {
    match error {
        ApplicationError::Internal(_) => NodeControlClientError::Rejected {
            status: 503,
            code: "secret_material_unavailable".into(),
            message: "Secret material is temporarily unavailable".into(),
            retryable: true,
        },
        ApplicationError::Invalid(_)
        | ApplicationError::NotFound(_)
        | ApplicationError::Conflict(_)
        | ApplicationError::Forbidden(_) => NodeControlClientError::Rejected {
            status: 403,
            code: "secret_material_forbidden".into(),
            message: "Secret material is not authorized".into(),
            retryable: false,
        },
    }
}

async fn persist_redacted_docker_logs(
    executor: &PostgresExecutor,
    node_id: NodeId,
    runtime: Arc<dyn RuntimeClient>,
    spec: &RuntimeUnitSpec,
    security_state_dir: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        spec.secrets
            .iter()
            .filter(|secret| !matches!(
                secret.target,
                a3s_runtime::contract::SecretTarget::RegistryCredential
            ))
            .count(),
        2
    );
    let query = RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor: None,
        limit: 32,
        stream: None,
    };
    let mut chunks = Vec::new();
    for attempt in 0..20 {
        chunks = runtime.logs(&query).await?;
        let stdout_ready = chunks.iter().any(|chunk| {
            chunk.stream == RuntimeLogStream::Stdout && chunk.data.contains("env-secret=[REDACTED]")
        });
        let stderr_ready = chunks.iter().any(|chunk| {
            chunk.stream == RuntimeLogStream::Stderr
                && chunk.data.contains("file-secret=[REDACTED]")
        });
        if stdout_ready && stderr_ready {
            break;
        }
        if attempt < 19 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    if chunks.iter().any(|chunk| {
        sensitive_plaintexts
            .iter()
            .any(|plaintext| chunk.data.contains(plaintext))
    }) || !chunks.iter().any(|chunk| {
        chunk.stream == RuntimeLogStream::Stdout && chunk.data.contains("env-secret=[REDACTED]")
    }) || !chunks.iter().any(|chunk| {
        chunk.stream == RuntimeLogStream::Stderr && chunk.data.contains("file-secret=[REDACTED]")
    }) {
        return Err(
            std::io::Error::other("real Docker Secret logs were not completely redacted").into(),
        );
    }

    let batch = NodeLogChunkBatch {
        schema: NodeLogChunkBatch::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        sent_at: Utc::now(),
        chunks: chunks
            .into_iter()
            .map(|chunk| NodeLogChunkReport {
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                checksum: format!("sha256:{:x}", Sha256::digest(chunk.data.as_bytes())),
                chunk,
            })
            .collect(),
        gaps: Vec::new(),
    };
    batch.validate()?;

    let first = record_log_batch(executor, security_state_dir, node_id, batch.clone()).await?;
    assert!(!first.replayed);
    assert_eq!(usize::from(first.accepted_chunks), batch.chunks.len());

    // Recreate the handler, repository, and object-store adapter to model a
    // control-plane restart after the batch receipt became durable.
    let replay = record_log_batch(executor, security_state_dir, node_id, batch).await?;
    assert!(replay.replayed);
    assert_eq!(replay.accepted_chunks, first.accepted_chunks);
    assert_log_objects_redacted(&security_state_dir.join("logs"), sensitive_plaintexts)?;
    Ok(())
}

async fn record_log_batch(
    executor: &PostgresExecutor,
    security_state_dir: &Path,
    node_id: NodeId,
    batch: NodeLogChunkBatch,
) -> Result<a3s_cloud_contracts::NodeLogChunkReceipt, Box<dyn std::error::Error>> {
    let nodes: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let objects: Arc<dyn ILogChunkStore> =
        Arc::new(LocalLogChunkStore::new(security_state_dir.join("logs"))?);
    RecordNodeLogChunksHandler::new(nodes, objects)
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch,
                received_at: Utc::now(),
            },
            context(),
        )
        .await?
        .map_err(|error| {
            std::io::Error::other(format!("could not persist real Docker log batch: {error}"))
                .into()
        })
}

fn assert_log_objects_redacted(
    root: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let marker = b"[REDACTED]";
    let mut directories = vec![root.to_path_buf()];
    let mut found_marker = false;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                let body = std::fs::read(entry.path())?;
                if sensitive_plaintexts.iter().any(|plaintext| {
                    let secret = plaintext.as_bytes();
                    !secret.is_empty() && body.windows(secret.len()).any(|window| window == secret)
                }) {
                    return Err(std::io::Error::other(
                        "plaintext Secret reached the durable log object store",
                    )
                    .into());
                }
                found_marker |= body.windows(marker.len()).any(|window| window == marker);
            }
        }
    }
    if !found_marker {
        return Err(
            std::io::Error::other("durable log objects contain no redaction evidence").into(),
        );
    }
    Ok(())
}

fn assert_tree_excludes_plaintext(
    root: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                let body = std::fs::read(entry.path())?;
                if sensitive_plaintexts.iter().any(|plaintext| {
                    let secret = plaintext.as_bytes();
                    !secret.is_empty() && body.windows(secret.len()).any(|window| window == secret)
                }) {
                    return Err(std::io::Error::other(
                        "plaintext Secret reached durable node state",
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn assert_secret_file_modes(
    root: &Path,
    expected: &[u32],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let mut directories = vec![root.to_path_buf()];
    let mut modes = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                modes.push(entry.metadata()?.permissions().mode() & 0o777);
            }
        }
    }
    modes.sort_unstable();
    assert_eq!(modes, expected);
    Ok(())
}

pub async fn exercise_pre_dispatch_cancellation(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let runtime = DeploymentFlowRuntime::new(
        workload_repository.clone(),
        test_artifact_resolver(),
        node_repository.clone(),
        node_repository,
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_id = OperationId::from_uuid(field_uuid(response, "operationId")?);
    let deployment_id = DeploymentId::from_uuid(field_uuid(response, "deploymentId")?);
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;

    for _ in 0..5 {
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Cancelled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let cancelled = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.node_id.is_none());
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("cancelled deployment operation has no projection")?
            .status,
        OperationStatus::Cancelled
    );
    Ok(())
}

pub async fn exercise_dispatched_cancellation(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if !docker_tests_enabled() {
        return Ok(());
    }

    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "update nodes set state = 'draining', aggregate_version = aggregate_version + 1 where organization_id = ",
            )
            .bind(organization_uuid)
            .append(" and state = 'ready'"),
        )
        .await?;
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&node_repository, organization_id).await?;
    let runtime = DeploymentFlowRuntime::new(
        workload_repository.clone(),
        test_artifact_resolver(),
        node_repository.clone(),
        node_repository.clone(),
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_id = OperationId::from_uuid(field_uuid(response, "operationId")?);
    let deployment_id = DeploymentId::from_uuid(field_uuid(response, "deploymentId")?);
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    let coordinator = FlowOperationCoordinator::new(
        reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?;

    for _ in 0..8 {
        coordinator.run_once().await?;
        if workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status
            == DeploymentStatus::Applying
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let applying = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(applying.status, DeploymentStatus::Applying);
    let apply_command_id = applying
        .command_id
        .ok_or("deployment has no apply command")?;
    let apply_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: 0,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let apply_command = apply_lease
        .commands
        .into_iter()
        .find(|command| command.command_id == apply_command_id.as_uuid())
        .ok_or("deployment apply command was not leased")?;
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } = &apply_command.payload
    else {
        return Err("deployment command is not Runtime apply".into());
    };
    let expected_spec = request.spec.clone();

    let state_directory = tempfile::tempdir()?;
    let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
        socket: docker_socket(),
        namespace: format!(
            "cloud-cancel-{}",
            &Uuid::now_v7().simple().to_string()[..12]
        ),
        operation_timeout_ms: 30_000,
        secret_memory_dir: docker_secret_memory_dir(),
    })?);
    driver.bind_node(node_id.as_uuid()).await?;
    let state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
        state_directory.path().join("runtime"),
    ));
    let runtime_driver: Arc<dyn RuntimeDriver> = driver;
    let runtime_client: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(state, runtime_driver));
    let command_executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(state_directory.path().join("journal"), node_id.as_uuid())?,
        runtime_client.clone(),
    );
    let apply_acknowledgement = command_executor.execute(apply_command.clone()).await?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        apply_acknowledgement,
    )
    .await?;

    let mut cancelling = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    let expected_version = cancelling.aggregate_version;
    let cancellation_at = Utc::now().max(cancelling.updated_at);
    cancelling.request_cancellation(cancellation_at)?;
    let cancellation_event =
        DeploymentCancellationRequested::envelope(&cancelling, Uuid::now_v7())?;
    workload_repository
        .request_deployment_cancellation(RequestDeploymentCancellationBundle {
            deployment: cancelling,
            expected_version,
            idempotency: IdempotencyRequest::new(
                format!("test.deployment.{deployment_id}.cancellation"),
                "cancel-after-runtime-apply",
                deployment_id.to_string().as_bytes(),
            )?,
            event: cancellation_event,
        })
        .await?;

    let mut cleanup_command_id = None;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::CleanupPending {
            cleanup_command_id = deployment.cleanup_command_id;
            break;
        }
    }
    let cleanup_command_id = cleanup_command_id.ok_or("cleanup command was not persisted")?;
    let cleanup_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: apply_command.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            Utc::now(),
            Utc::now() + ChronoDuration::seconds(10),
        )
        .await?;
    let cleanup_command = cleanup_lease
        .commands
        .into_iter()
        .find(|command| command.command_id == cleanup_command_id.as_uuid())
        .ok_or("Runtime stop command was not leased")?;
    assert!(matches!(
        &cleanup_command.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    let cleanup_acknowledgement = command_executor.execute(cleanup_command).await?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities,
        cleanup_acknowledgement,
    )
    .await?;

    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Cancelled {
            break;
        }
    }

    let cancelled = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert_eq!(cancelled.command_id, Some(apply_command_id));
    assert_eq!(cancelled.cleanup_command_id, Some(cleanup_command_id));
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("cancelled deployment operation has no projection")?
            .status,
        OperationStatus::Cancelled
    );
    match runtime_client.inspect(&expected_spec.unit_id).await? {
        RuntimeInspection::Found { observation, .. } => {
            assert_eq!(observation.state, RuntimeUnitState::Stopped)
        }
        RuntimeInspection::NotFound { .. } => {}
    }
    assert_eq!(
        Database::new(PostgresDialect, executor.clone())
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where correlation_id = ")
                    .bind(operation_id.as_uuid())
                    .append(" and acknowledgement is not null"),
            )
            .await?,
        2
    );
    runtime_client
        .remove(&RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("integration-cleanup-{}", Uuid::now_v7()),
            unit_id: expected_spec.unit_id,
            generation: expected_spec.generation,
            deadline_at_ms: None,
        })
        .await?;
    Ok(())
}

fn test_artifact_resolver() -> Arc<dyn IOciArtifactResolver> {
    Arc::new(ExpectedDigestArtifactResolver)
}

fn deployment_artifact_resolver(
    executor: &PostgresExecutor,
    security_state_dir: &Path,
) -> Result<Arc<dyn IOciArtifactResolver>, Box<dyn std::error::Error>> {
    let Some(uri) = std::env::var("A3S_CLOUD_TEST_PRIVATE_REGISTRY_ARTIFACT").ok() else {
        return Ok(test_artifact_resolver());
    };
    let reference = OciArtifactReference {
        uri,
        expected_digest: None,
    };
    let (registry, _) = reference.registry_and_repository()?;
    let secrets: Arc<dyn ISecretRepository> =
        Arc::new(PostgresSecretRepository::new(executor.clone()));
    let encryption: Arc<dyn ISecretEncryptionService> = Arc::new(
        LocalKeyEncryptionService::load_or_create(security_state_dir.join("key-encryption.key"))?,
    );
    Ok(Arc::new(
        OciRegistryArtifactResolver::new(Duration::from_secs(10), [registry.to_owned()])?
            .with_registry_secret_material(secrets, encryption),
    ))
}

struct ExpectedDigestArtifactResolver;

#[async_trait]
impl IOciArtifactResolver for ExpectedDigestArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        _registry_credential: Option<
            &a3s_cloud_control_plane::modules::workloads::OciRegistryCredentialReference,
        >,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        let digest = reference
            .expected_digest
            .clone()
            .or_else(|| reference.bound_digest().ok().flatten().map(str::to_owned))
            .ok_or_else(|| {
                OciArtifactResolutionError::Registry(
                    "test resolver requires an expected digest".into(),
                )
            })?;
        let repository = reference
            .repository()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        Ok(OciArtifact {
            uri: format!("oci://{repository}@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        })
    }
}

async fn persist_command_result(
    repository: &Arc<PostgresNodeRepository>,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: RuntimeCapabilities,
    acknowledgement: NodeCommandAck,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_at = acknowledgement.completed_at;
    let observations = acknowledgement_observation(&acknowledgement)
        .map(|observation| {
            vec![RuntimeObservationReport {
                report_id: acknowledgement.command_id,
                command_id: Some(acknowledgement.command_id),
                observed_at,
                observation,
            }]
        })
        .unwrap_or_default();
    let sent_at = Utc::now().max(observed_at);
    repository
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
                observations,
            },
            observed_at,
        )
        .await?;
    assert!(
        !repository
            .acknowledge_command(acknowledgement, sent_at)
            .await?
            .replayed
    );
    Ok(())
}

fn acknowledgement_observation(acknowledgement: &NodeCommandAck) -> Option<RuntimeObservation> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeApplied { observation } => Some(observation.as_ref().clone()),
            NodeCommandResult::RuntimeStopped {
                inspection: RuntimeInspection::Found { observation, .. },
            } => Some(observation.as_ref().clone()),
            NodeCommandResult::RuntimeInspected { .. }
            | NodeCommandResult::RuntimeStopped { .. }
            | NodeCommandResult::RuntimeRemoved { .. }
            | NodeCommandResult::GatewaySnapshotInstalled { .. } => None,
        },
        NodeCommandOutcome::Rejected { .. } | NodeCommandOutcome::Failed { .. } => None,
    }
}

async fn ready_node(
    repository: &Arc<PostgresNodeRepository>,
    organization_id: OrganizationId,
) -> Result<(NodeId, Uuid, RuntimeCapabilities), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let unique = Uuid::now_v7().simple().to_string();
    let node_name = format!("deployment-flow-{}", &unique[..12]);
    let secret = format!("a3sn_{unique}{unique}");
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        node_name.clone(),
        credential.clone(),
        now,
        now + ChronoDuration::minutes(5),
    )?;
    repository
        .issue_enrollment_token(
            token.clone(),
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "fleet.enrollment-token.issued".into(),
                schema_version: 1,
                organization_id: organization_id.as_uuid(),
                aggregate_id: token.id.as_uuid(),
                aggregate_version: token.aggregate_version,
                occurred_at: now,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: serde_json::json!({"name": node_name.clone()}),
            },
            IdempotencyRequest::new(
                "test.deployment-flow.enrollment",
                node_name.clone(),
                node_name.as_bytes(),
            )?,
        )
        .await?;
    let capabilities = runtime_capabilities();
    let stored_capabilities = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(&capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = repository
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new(node_name)?,
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: stored_capabilities.clone(),
                request_digest: format!("sha256:{}", "1".repeat(64)),
                requested_at: now,
            },
        )
        .await?;
    repository
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored_capabilities,
            observed_at: now + ChronoDuration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id, capabilities))
}

fn healthy_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "integration clock predates Unix epoch")?;
    let spec_digest = spec.digest()?;
    let endpoint_claims = spec
        .network
        .ports
        .iter()
        .filter(|port| port.protocol == TransportProtocol::Tcp)
        .enumerate()
        .map(|(index, port)| {
            let host_port = 49_152_u16
                .checked_add(u16::try_from(index).map_err(|_| {
                    "integration Runtime observation has too many service ports".to_owned()
                })?)
                .ok_or_else(|| {
                    "integration Runtime observation service port range overflowed".to_owned()
                })?;
            let endpoint = RuntimeServiceEndpoint::node_local_http(&port.name, host_port)?;
            Ok((endpoint.claim_key(), endpoint.origin))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("integration-container".into()),
        provider_build: Some("integration-runtime-1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: now_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "integration-runtime-1".into(),
            spec_digest,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            claims: endpoint_claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("integration-runtime")
            .expect("valid integration provider ID"),
        provider_build: "integration-runtime-1".into(),
        unit_classes: vec![RuntimeUnitClass::Service],
        artifact_media_types: vec![
            "application/vnd.oci.image.manifest.v1+json".into(),
            "application/vnd.docker.distribution.manifest.v2+json".into(),
        ],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: vec![HealthCheckKind::Http],
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
            RuntimeFeature::SecretReferences,
        ],
    }
}

fn field_uuid(value: &Value, field: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(Uuid::parse_str(value[field].as_str().ok_or_else(
        || format!("workload response omitted {field}"),
    )?)?)
}

fn docker_tests_enabled() -> bool {
    std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref() == Ok("1")
}

fn docker_socket() -> String {
    std::env::var("A3S_CLOUD_TEST_DOCKER_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".into())
}

fn docker_secret_memory_dir() -> PathBuf {
    std::env::var_os("A3S_CLOUD_TEST_SECRET_MEMORY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev/shm/a3s-cloud/test-secrets"))
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
