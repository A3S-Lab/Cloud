use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandResult, NodeHeartbeat, NodeObservationBatch, NodeResourceInventory,
    NodeResourceSlot, ResourceAllocation, ResourceKind, ResourceUnit, RuntimeObservationReport,
    RuntimeServiceEndpoint,
};
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::fleet::domain::entities::EnrollmentToken;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::{LocalKeyEncryptionService, PostgresNodeRepository};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, OperationStatus,
    PostgresOperationRepository, ReconcileOperationsHandler,
};
use a3s_cloud_control_plane::modules::secrets::{
    ISecretEncryptionService, ISecretRepository, PostgresSecretRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnrollmentTokenId, IdempotencyRequest, NodeId, OperationId, OrganizationId,
    ResourceClaimId,
};
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentFlowConfig, DeploymentFlowDependencies, DeploymentFlowRuntime, DeploymentStatus,
    IOciArtifactResolver, IResourceClaimRepository, IWorkloadRepository, IWorkloadRuntimeControl,
    IWorkloadRuntimeTargetRepository, OciArtifact, OciArtifactReference,
    OciArtifactResolutionError, OciRegistryArtifactResolver, PostgresResourceClaimRepository,
    PostgresWorkloadRepository, WorkloadRuntimeReconciler,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeEvidence, RuntimeFeature, RuntimeHealthObservation, RuntimeHealthState,
    RuntimeInspection, RuntimeObservation, RuntimeUnitClass, RuntimeUnitState, TransportProtocol,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[path = "deployment_flow/cancellation.rs"]
mod cancellation;

pub use cancellation::exercise_pre_dispatch_cancellation;

#[derive(Clone)]
pub struct DeploymentFlowFixture {
    pub node_id: NodeId,
    pub agent_instance_id: Uuid,
    pub capabilities: RuntimeCapabilities,
    pub after_sequence: u64,
}

pub async fn exercise_deployment_flow(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    response: &Value,
    security_state_dir: &Path,
    sensitive_plaintexts: &[&str],
) -> Result<DeploymentFlowFixture, Box<dyn std::error::Error>> {
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
    let (node_id, agent_instance_id, capabilities, _inventory) =
        ready_node(&node_repository, organization_id).await?;
    let workloads: Arc<dyn IWorkloadRepository> = workload_repository.clone();
    let nodes: Arc<dyn INodeRepository> = node_repository.clone();
    let node_control: Arc<dyn INodeControlRepository> = node_repository.clone();
    let resource_claims = Arc::new(PostgresResourceClaimRepository::new(executor.clone()));
    let runtime = DeploymentFlowRuntime::new(
        DeploymentFlowDependencies::new(
            workloads,
            resource_claims.clone(),
            deployment_artifact_resolver(executor, security_state_dir)?,
            nodes,
            node_control,
            Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ),
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

    let mut reconciled_before_prepare = 0;
    for _ in 0..8 {
        let cycle = coordinator.run_once().await?;
        reconciled_before_prepare += cycle.reconciled_before_work;
        if workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status
            == DeploymentStatus::Scheduled
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(reconciled_before_prepare > 0);
    let scheduled = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    if scheduled.status != DeploymentStatus::Scheduled {
        let snapshot = flow.engine().snapshot(&operation_id.to_string()).await?;
        return Err(format!(
            "deployment did not reach scheduled before resource preparation; deployment_status={}; flow_status={:?}; flow_sequence={}; waits={:?}; steps={:?}",
            scheduled.status.as_str(),
            snapshot.status,
            snapshot.last_sequence,
            snapshot.waits,
            snapshot.steps
        )
        .into());
    }
    for _ in 0..16 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        if resource_claims
            .find(
                organization_id,
                ResourceClaimId::from_uuid(deployment_id.as_uuid()),
            )
            .await?
            .prepare_command_id
            .is_some()
        {
            break;
        }
    }
    let claim_before_prepare = resource_claims
        .find(
            organization_id,
            ResourceClaimId::from_uuid(deployment_id.as_uuid()),
        )
        .await?;
    if claim_before_prepare.prepare_command_id.is_none() {
        let snapshot = flow.engine().snapshot(&operation_id.to_string()).await?;
        return Err(format!(
            "deployment resource preparation was not dispatched; claim_state={}; flow_status={:?}",
            claim_before_prepare.state.as_str(),
            snapshot.status
        )
        .into());
    }
    let now = Utc::now();
    let preparation_lease = node_repository
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
    let preparation = preparation_lease
        .commands
        .into_iter()
        .find(|command| {
            matches!(
                command.payload,
                a3s_cloud_contracts::NodeCommandPayload::ResourceClaimPrepare { .. }
            )
        })
        .ok_or("deployment resource preparation command was not leased")?;
    let preparation_acknowledgement = resource_claim_acknowledgement(&preparation)?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        preparation_acknowledgement,
    )
    .await?;

    for _ in 0..12 {
        tokio::time::sleep(Duration::from_millis(6)).await;
        coordinator.run_once().await?;
        if workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status
            == DeploymentStatus::Applying
        {
            break;
        }
    }
    let applying = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(applying.status, DeploymentStatus::Applying);
    let command_id = applying.command_id.ok_or("deployment has no command")?;
    let now = Utc::now();
    let apply_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: preparation.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + ChronoDuration::seconds(10),
        )
        .await?;
    let command = apply_lease
        .commands
        .into_iter()
        .find(|command| command.command_id == command_id.as_uuid())
        .ok_or("deployment command was not leased")?;
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request, .. } = &command.payload
    else {
        return Err("deployment command is not Runtime apply".into());
    };
    let serialized_command = serde_json::to_string(&command)?;
    assert!(sensitive_plaintexts
        .iter()
        .all(|plaintext| !serialized_command.contains(plaintext)));
    let acknowledgement =
        runtime_apply_acknowledgement(&command, healthy_observation(&request.spec)?)?;
    persist_command_result(
        &node_repository,
        node_id,
        agent_instance_id,
        capabilities.clone(),
        acknowledgement,
    )
    .await?;
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
        DeploymentFlowDependencies::new(
            workload_repository.clone(),
            Arc::new(PostgresResourceClaimRepository::new(executor.clone())),
            deployment_artifact_resolver(executor, security_state_dir)?,
            node_repository.clone(),
            node_repository.clone(),
            Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ),
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
        Arc::new(PostgresResourceClaimRepository::new(executor.clone())),
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
        resource_claim: recovery_binding,
    } = &recovery_command.payload
    else {
        return Err("workload reconciliation recovery is not Runtime apply".into());
    };
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply {
        resource_claim: original_binding,
        ..
    } = &command.payload
    else {
        unreachable!("validated deployment Runtime apply");
    };
    assert_eq!(recovery_binding, original_binding);
    assert!(recovery_binding.is_some());
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
        runtime_apply_acknowledgement(
            recovery_command,
            healthy_observation(&recovery_request.spec)?,
        )?,
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
    Ok(DeploymentFlowFixture {
        node_id,
        agent_instance_id,
        capabilities,
        after_sequence: recovery_command.sequence,
    })
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

pub(super) async fn persist_command_result(
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
            }
            .into(),
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

pub(super) fn resource_claim_acknowledgement(
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
) -> Result<NodeCommandAck, Box<dyn std::error::Error>> {
    let completed_at = Utc::now().max(command.issued_at);
    let result = match &command.payload {
        a3s_cloud_contracts::NodeCommandPayload::ResourceClaimPrepare { request } => {
            NodeCommandResult::ResourceClaimPrepared {
                prepared: a3s_cloud_contracts::NodeResourceClaimPrepared::new(
                    request,
                    completed_at,
                )?,
            }
        }
        a3s_cloud_contracts::NodeCommandPayload::ResourceClaimRelease { request } => {
            NodeCommandResult::ResourceClaimReleased {
                released: a3s_cloud_contracts::NodeResourceClaimReleased::new(
                    request,
                    completed_at,
                )?,
            }
        }
        _ => return Err("node command is not a resource Claim command".into()),
    };
    Ok(successful_acknowledgement(command, result, completed_at))
}

pub(super) fn runtime_apply_acknowledgement(
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    mut observation: RuntimeObservation,
) -> Result<NodeCommandAck, Box<dyn std::error::Error>> {
    let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply {
        request,
        resource_claim,
    } = &command.payload
    else {
        return Err("node command is not Runtime apply".into());
    };
    observation.validate_against(&request.spec)?;
    if let Some(binding) = resource_claim {
        binding.bind_runtime_observation(&mut observation)?;
    }
    let completed_at = Utc::now().max(command.issued_at);
    Ok(successful_acknowledgement(
        command,
        NodeCommandResult::RuntimeApplied {
            observation: Box::new(observation),
        },
        completed_at,
    ))
}

fn successful_acknowledgement(
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    result: NodeCommandResult,
    completed_at: chrono::DateTime<Utc>,
) -> NodeCommandAck {
    NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(result),
        },
    }
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
            | NodeCommandResult::ResourceClaimPrepared { .. }
            | NodeCommandResult::ResourceClaimReleased { .. }
            | NodeCommandResult::GatewaySnapshotInstalled { .. }
            | NodeCommandResult::GatewaySnapshotObserved { .. }
            | NodeCommandResult::BoxBuildStarted { .. }
            | NodeCommandResult::BoxBuildInspected { .. }
            | NodeCommandResult::BoxBuildCancelled { .. }
            | NodeCommandResult::BoxBuildRemoved { .. }
            | NodeCommandResult::CodeAgentCommandAccepted { .. }
            | NodeCommandResult::PluginHostCapabilitiesInspected { .. }
            | NodeCommandResult::PluginHostPlanned { .. }
            | NodeCommandResult::PluginHostApplied { .. }
            | NodeCommandResult::PluginHostEnablementPlanned { .. }
            | NodeCommandResult::PluginHostObserved { .. } => None,
        },
        NodeCommandOutcome::Rejected { .. } | NodeCommandOutcome::Failed { .. } => None,
    }
}

async fn ready_node(
    repository: &Arc<PostgresNodeRepository>,
    organization_id: OrganizationId,
) -> Result<(NodeId, Uuid, RuntimeCapabilities, NodeResourceInventory), Box<dyn std::error::Error>>
{
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
    let inventory = NodeResourceInventory::new(
        reservation.node.id.as_uuid(),
        agent_instance_id,
        1,
        now + ChronoDuration::milliseconds(1),
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
    repository
        .record_resource_inventory(inventory.clone(), now + ChronoDuration::milliseconds(2))
        .await?;
    repository
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored_capabilities,
            observed_at: now + ChronoDuration::milliseconds(3),
        })
        .await?;
    Ok((
        reservation.node.id,
        agent_instance_id,
        capabilities,
        inventory,
    ))
}

pub(super) fn healthy_observation(
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
            let endpoint = RuntimeServiceEndpoint::node_local_tcp(&port.name, host_port)?;
            Ok((endpoint.claim_key(), endpoint.claim_value()))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("box-execution-integration".into()),
        provider_build: Some("a3s-box-integration".into()),
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
            provider_build: "a3s-box-integration".into(),
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
        provider_id: a3s_runtime::ProviderId::parse("a3s-box-integration")
            .expect("valid Box integration provider ID"),
        provider_build: "a3s-box-integration".into(),
        unit_classes: vec![RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
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
            RuntimeFeature::ServiceTcp,
            RuntimeFeature::SecretReferences,
        ],
    }
}

fn field_uuid(value: &Value, field: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(Uuid::parse_str(value[field].as_str().ok_or_else(
        || format!("workload response omitted {field}"),
    )?)?)
}
