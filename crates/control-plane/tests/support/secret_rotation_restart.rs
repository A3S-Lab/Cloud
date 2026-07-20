use crate::deployment_flow_support::{
    healthy_observation, persist_command_result, DeploymentFlowFixture,
};
use a3s_cloud_contracts::{
    CloudSecretReference, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::NodeCapabilities;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, OperationStatus,
    PostgresOperationRepository, ReconcileOperationsHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, SecretId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentFlowConfig, DeploymentFlowRuntime, DeploymentStatus, IOciArtifactResolver,
    ISecretRotationRestartRepository, IWorkloadRepository, OciArtifact, OciArtifactReference,
    OciArtifactResolutionError, OciRegistryCredentialReference, PostgresWorkloadRepository,
    SecretRotationRestartReconciler,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::RuntimeInspection;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct SecretRotationRestartFixture {
    pub revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
}

#[allow(clippy::too_many_arguments)]
pub async fn exercise_secret_rotation_restart(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_uuid: Uuid,
    workload_uuid: Uuid,
    secret_uuid: Uuid,
    version: u64,
    node: &DeploymentFlowFixture,
    sensitive_plaintexts: &[&str],
) -> Result<SecretRotationRestartFixture, Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload_id = WorkloadId::from_uuid(workload_uuid);
    let secret_id = SecretId::from_uuid(secret_uuid);
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let source_workload = workload_repository
        .find_workload(organization_id, workload_id)
        .await?;
    let source_revision_id = source_workload
        .active_revision_id
        .ok_or("Secret rotation fixture workload has no active revision")?;
    let source_revision = workload_repository
        .find_revision(organization_id, source_revision_id)
        .await?;
    assert!(source_revision
        .request
        .secrets
        .iter()
        .filter(|binding| binding.secret_id == secret_id)
        .all(|binding| binding.version < version));

    let first_port: Arc<dyn ISecretRotationRestartRepository> =
        Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let second_port: Arc<dyn ISecretRotationRestartRepository> =
        Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let first =
        SecretRotationRestartReconciler::new(first_port, Duration::from_millis(5), 100, 100)?;
    let second =
        SecretRotationRestartReconciler::new(second_port, Duration::from_millis(5), 100, 100)?;

    // Both reconstructed workers can observe the committed rotation. The
    // PostgreSQL advisory lock and durable checkpoint permit only one derived
    // revision/deployment transaction.
    let reconciled_at = Utc::now();
    let (left, right) = tokio::join!(
        first.run_once(reconciled_at),
        second.run_once(reconciled_at)
    );
    let left = left?;
    let right = right?;
    assert!(left.failures.is_empty(), "{:?}", left.failures);
    assert!(right.failures.is_empty(), "{:?}", right.failures);
    assert_eq!(
        left.scheduled_restarts + right.scheduled_restarts,
        1,
        "concurrent Secret rotation reconciliation scheduled duplicates"
    );

    let database = Database::new(PostgresDialect, executor.clone());
    let (
        secret_event_id,
        stored_source_revision_id,
        target_revision_id,
        deployment_id,
        operation_id,
    ) = database
        .fetch_one_as(
            sql_query::<(Uuid, Uuid, Uuid, Uuid, Uuid)>(
                "select secret_event_id, source_revision_id, target_revision_id, deployment_id, operation_id from secret_rotation_restarts where organization_id = ",
            )
            .bind(organization_uuid)
            .append(" and secret_id = ")
            .bind(secret_uuid)
            .append(" and secret_version = ")
            .bind(version)
            .append(" and workload_id = ")
            .bind(workload_uuid),
        )
        .await?;
    assert_eq!(stored_source_revision_id, source_revision_id.as_uuid());
    let target_revision_id = WorkloadRevisionId::from_uuid(target_revision_id);
    let deployment_id = DeploymentId::from_uuid(deployment_id);
    let operation_id = OperationId::from_uuid(operation_id);

    let revisions = workload_repository
        .list_revisions(organization_id, workload_id)
        .await?;
    assert_eq!(revisions.len(), 2);
    let target_revision = workload_repository
        .find_revision(organization_id, target_revision_id)
        .await?;
    assert_eq!(target_revision.generation, source_revision.generation + 1);
    assert_eq!(
        target_revision
            .resolved_template()
            .expect("resolved Secret restart revision")
            .artifact,
        source_revision
            .resolved_template()
            .expect("resolved source revision")
            .artifact
    );
    assert!(target_revision
        .request
        .secrets
        .iter()
        .filter(|binding| binding.secret_id == secret_id)
        .all(|binding| binding.version == version));
    assert_ne!(
        target_revision.request_digest,
        source_revision.request_digest
    );
    assert_ne!(
        target_revision.template_digest,
        source_revision.template_digest
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from outbox_events where event_key = 'workload.deployment.requested' and causation_id = ",
                )
                .bind(secret_event_id),
            )
            .await?,
        1
    );
    assert_eq!(
        first.run_once(Utc::now()).await?.inspected_rotations,
        0,
        "reconstructed restart reconciler did not honor its checkpoint"
    );

    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    node_repository
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: node.node_id,
            agent_instance_id: node.agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: NodeCapabilities::new(
                node.capabilities.provider_id.to_string(),
                node.capabilities.provider_build.clone(),
                serde_json::to_value(&node.capabilities)?,
            )?,
            observed_at: Utc::now(),
        })
        .await?;
    let flow = restart_flow(
        postgres_url,
        workload_repository.clone(),
        node_repository.clone(),
    )
    .await?;
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let coordinator = build_coordinator(&flow, operation_repository.clone())?;
    for _ in 0..12 {
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
    let command_id = applying
        .command_id
        .ok_or("Secret rotation deployment has no Runtime apply command")?;
    let now = Utc::now();
    let lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node.node_id.as_uuid(),
                agent_instance_id: node.agent_instance_id,
                after_sequence: node.after_sequence,
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
        .ok_or("Secret rotation Runtime apply command was not leased")?;
    let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err("Secret rotation command is not Runtime apply".into());
    };
    assert_eq!(request.spec.generation, target_revision.generation);
    assert_eq!(
        request.spec.artifact.digest,
        source_revision
            .resolved_template()
            .expect("source artifact")
            .artifact
            .digest
    );
    let references = request
        .spec
        .secrets
        .iter()
        .map(|binding| CloudSecretReference::parse(&binding.reference))
        .collect::<Result<Vec<_>, _>>()?;
    let rotated_references = references
        .iter()
        .filter(|reference| reference.secret_id == secret_uuid)
        .collect::<Vec<_>>();
    assert_eq!(rotated_references.len(), 2);
    assert!(rotated_references.iter().all(|reference| {
        reference.workload_revision_id == target_revision_id.as_uuid()
            && reference.version == version
    }));
    let serialized_command = serde_json::to_string(command)?;
    assert!(sensitive_plaintexts
        .iter()
        .all(|plaintext| !serialized_command.contains(plaintext)));

    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: Utc::now(),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeApplied {
                observation: Box::new(healthy_observation(&request.spec)?),
            }),
        },
    };
    persist_command_result(
        &node_repository,
        node.node_id,
        node.agent_instance_id,
        node.capabilities.clone(),
        acknowledgement,
    )
    .await?;

    // Lose the coordinator after the restart result is durable, reconstruct it,
    // and require the same operation to activate before issuing one deterministic
    // stop for the previous immutable Runtime revision.
    drop(coordinator);
    drop(flow);
    let flow = restart_flow(
        postgres_url,
        workload_repository.clone(),
        node_repository.clone(),
    )
    .await?;
    let coordinator = build_coordinator(&flow, operation_repository.clone())?;
    for _ in 0..12 {
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Retiring
            && deployment.retirement_command_id.is_some()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let retiring = workload_repository
        .find_deployment(organization_id, deployment_id)
        .await?;
    assert_eq!(retiring.status, DeploymentStatus::Retiring);
    let retirement_command_id = retiring
        .retirement_command_id
        .ok_or("Secret rotation update has no previous Runtime retirement command")?;
    assert_eq!(
        workload_repository
            .find_workload(organization_id, workload_id)
            .await?
            .active_revision_id,
        Some(target_revision_id)
    );
    let now = Utc::now();
    let retirement_lease = node_repository
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node.node_id.as_uuid(),
                agent_instance_id: node.agent_instance_id,
                after_sequence: command.sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + ChronoDuration::seconds(10),
        )
        .await?;
    let retirement_command = retirement_lease
        .commands
        .iter()
        .find(|candidate| candidate.command_id == retirement_command_id.as_uuid())
        .ok_or("Secret rotation previous Runtime retirement command was not leased")?;
    let NodeCommandPayload::RuntimeStop {
        request: retirement_request,
    } = &retirement_command.payload
    else {
        return Err("Secret rotation retirement command is not Runtime stop".into());
    };
    let source_spec =
        a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec(
            &source_revision,
        )?;
    assert_eq!(retirement_request.unit_id, source_spec.unit_id);
    assert_eq!(retirement_request.generation, source_spec.generation);
    persist_command_result(
        &node_repository,
        node.node_id,
        node.agent_instance_id,
        node.capabilities.clone(),
        NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: retirement_command.command_id,
            lease_id: retirement_command.lease_id,
            node_id: retirement_command.node_id,
            sequence: retirement_command.sequence,
            payload_digest: retirement_command.payload_digest.clone(),
            completed_at: Utc::now(),
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::NotFound {
                        schema: RuntimeInspection::SCHEMA.into(),
                        unit_id: source_spec.unit_id,
                        last_generation: Some(source_spec.generation),
                    },
                }),
            },
        },
    )
    .await?;
    for _ in 0..12 {
        coordinator.run_once().await?;
        let deployment = workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Active
            && operation_repository
                .find_projection(operation_id)
                .await?
                .is_some_and(|projection| projection.status == OperationStatus::Succeeded)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        workload_repository
            .find_deployment(organization_id, deployment_id)
            .await?
            .status,
        DeploymentStatus::Active
    );
    assert_eq!(
        workload_repository
            .find_workload(organization_id, workload_id)
            .await?
            .active_revision_id,
        Some(target_revision_id)
    );
    assert_eq!(
        operation_repository
            .find_projection(operation_id)
            .await?
            .ok_or("Secret rotation operation has no projection")?
            .status,
        OperationStatus::Succeeded
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where payload::text like ",)
                    .bind(format!("%{}%", target_revision_id)),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where correlation_id = ",)
                    .bind(operation_id.as_uuid())
                    .append(" and command_kind = 'runtime_stop'"),
            )
            .await?,
        1
    );

    Ok(SecretRotationRestartFixture {
        revision_id: target_revision_id,
        deployment_id,
        operation_id,
    })
}

async fn restart_flow(
    postgres_url: &str,
    workloads: Arc<PostgresWorkloadRepository>,
    nodes: Arc<PostgresNodeRepository>,
) -> Result<FlowInfrastructure, Box<dyn std::error::Error>> {
    let runtime = DeploymentFlowRuntime::new(
        workloads,
        Arc::new(ResolvedRevisionOnly),
        nodes.clone(),
        nodes,
        Arc::new(a3s_cloud_control_plane::modules::workloads::UnroutedDeploymentRouteUpdater),
        ChronoDuration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 5, 20_000, 5_000, 5, 20_000)?,
    )?;
    Ok(FlowInfrastructure::connect(postgres_url, Arc::new(runtime)).await?)
}

fn build_coordinator(
    flow: &FlowInfrastructure,
    operations: Arc<dyn IOperationRepository>,
) -> Result<FlowOperationCoordinator, Box<dyn std::error::Error>> {
    let reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operations,
            Arc::new(FlowOperationEngine::new(flow.engine())),
        )),
        Duration::from_millis(5),
        100,
    );
    Ok(FlowOperationCoordinator::new(
        reconciler,
        flow,
        Duration::from_millis(5),
        Duration::from_secs(1),
    )?)
}

struct ResolvedRevisionOnly;

#[async_trait]
impl IOciArtifactResolver for ResolvedRevisionOnly {
    async fn resolve(
        &self,
        _reference: &OciArtifactReference,
        _registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        Err(OciArtifactResolutionError::Registry(
            "Secret rotation restart unexpectedly re-resolved its pinned artifact".into(),
        ))
    }
}
