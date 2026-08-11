use a3s_cloud_contracts::NodeCommandPayload;
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId,
    OrganizationId, ProjectId, RepositoryError, ResourceName, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_control_plane::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentRequested, DeploymentStatus, HttpHealthCheck,
    IWorkloadRepository, OciArtifact, PostgresWorkloadRepository, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate, Workload, WorkloadControlSpec, WorkloadReplicaLifecycle,
    WorkloadRevision,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::RuntimeApplyRequest;
use chrono::{Duration, Timelike, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct WorkloadFixture {
    pub workload_id: WorkloadId,
    pub deployment_id: DeploymentId,
    pub revision_id: WorkloadRevisionId,
    pub revision_generation: u64,
    pub candidate_revision_id: WorkloadRevisionId,
    pub candidate_generation: u64,
    pub candidate_deployment_id: DeploymentId,
    pub node_id: NodeId,
}

pub async fn exercise_workloads(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
) -> Result<WorkloadFixture, Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let project_id = ProjectId::from_uuid(project_uuid);
    let environment_id = EnvironmentId::from_uuid(environment_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("sub-microsecond workload timestamp");
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("HTTP fixture")?,
        now,
    );
    let first_request = request(workload, 1, 'a', "deploy-http-fixture", now)?;
    let first_deployment_id = first_request.deployment.id;
    let first_revision_id = first_request.revision.id;
    let first_operation_id = first_request.operation.id;
    let (first, replay) = tokio::join!(
        repository.create_deployment(first_request.clone()),
        repository.create_deployment(first_request.clone())
    );
    let first = first?;
    let replay = replay?;
    assert_ne!(first.replayed, replay.replayed);
    assert_eq!(first.deployment.id, replay.deployment.id);
    assert_eq!(
        first
            .revision
            .resolved_template()
            .expect("stored revision is resolved")
            .artifact
            .digest,
        digest('a')
    );
    assert!(matches!(
        repository
            .find_workload(OrganizationId::new(), first.workload.id)
            .await,
        Err(RepositoryError::NotFound)
    ));
    let replica_id = WorkloadReplicaId::from_uuid(first.workload.id.as_uuid());
    let member_id = WorkloadReplicaMemberId::from_uuid(first.workload.id.as_uuid());
    let control = repository
        .find_workload_control(organization_id, first.workload.id)
        .await?;
    assert_eq!(control.spec.managed_owner, None);
    assert_eq!(control.spec.placement_policy.desired_replicas(), 1);
    let replica = repository
        .find_workload_replica(organization_id, first.workload.id, replica_id)
        .await?;
    assert_eq!(replica.generation, 1);
    assert_eq!(replica.revision_id, first_revision_id);
    let member = repository
        .find_workload_replica_member(organization_id, replica_id, member_id)
        .await?;
    assert_eq!(member.node_id, None);
    assert_eq!(member.placement_generation, 0);
    let first_binding = repository
        .find_deployment_replica_binding(organization_id, first_deployment_id)
        .await?;
    assert_eq!(first_binding.replica_id, replica_id);
    assert_eq!(first_binding.member_id, member_id);
    assert_eq!(first_binding.replica_generation, 1);
    assert_eq!(
        first_binding.runtime_unit_id,
        first.revision.runtime_unit_id()
    );

    let mut changed_idempotency = first_request.clone();
    changed_idempotency.idempotency = IdempotencyRequest::new(
        "workload.deploy",
        "deploy-http-fixture",
        b"different canonical request",
    )?;
    assert!(matches!(
        repository.create_deployment(changed_idempotency).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workloads where id = ")
                    .bind(first.workload.id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workload_revisions where id = ")
                    .bind(first_revision_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from operation_requests where operation_id = ")
                    .bind(first_operation_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ")
                    .bind(first_deployment_id.as_uuid()),
            )
            .await?,
        1
    );

    let node_uuid = database
        .fetch_one_as(
            sql_query::<Uuid>(
                "select nodes.id from nodes join node_resource_inventory_heads on node_resource_inventory_heads.organization_id = nodes.organization_id and node_resource_inventory_heads.node_id = nodes.id where nodes.organization_id = ",
            )
                .bind(organization_uuid)
                .append(" order by nodes.id asc limit 1"),
        )
        .await?;
    let node_id = NodeId::from_uuid(node_uuid);
    let command_id = NodeCommandId::from_uuid(first_deployment_id.as_uuid());
    let command_issued_at = now + Duration::seconds(2);
    let command_deadline = command_issued_at + Duration::minutes(5);
    let command = PostgresNodeRepository::new(executor.clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: first.workload.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{first_deployment_id}:apply"),
                    deadline_at_ms: Some(u64::try_from(command_deadline.timestamp_millis())?),
                    spec: project_runtime_spec(&first.revision)?,
                }),
                resource_claim: None,
            },
            issued_at: command_issued_at,
            not_after: command_deadline,
            correlation_id: first.operation.id.as_uuid(),
        })
        .await?;
    assert!(!command.replayed);
    assert_eq!(command.value.id, command_id);

    let resolving = repository
        .mark_resolving(first_deployment_id, 1, now + Duration::seconds(1))
        .await?;
    assert_eq!(resolving.status, DeploymentStatus::Resolving);
    assert_eq!(resolving.updated_at.nanosecond() % 1_000, 0);
    assert_eq!(
        repository
            .mark_resolving(first_deployment_id, 1, now + Duration::seconds(1))
            .await?,
        resolving
    );
    let scheduled = repository
        .assign_node(
            first_deployment_id,
            resolving.aggregate_version,
            node_id,
            now + Duration::seconds(2),
        )
        .await?;
    let placed_member = repository
        .find_workload_replica_member(organization_id, replica_id, member_id)
        .await?;
    assert_eq!(placed_member.node_id, Some(node_id));
    assert_eq!(placed_member.placement_generation, 1);
    let placed_binding = repository
        .find_deployment_replica_binding(organization_id, first_deployment_id)
        .await?;
    assert_eq!(placed_binding.node_id, Some(node_id));
    assert_eq!(placed_binding.placement_generation, 1);
    assert!(matches!(
        repository
            .assign_node(
                first_deployment_id,
                resolving.aggregate_version,
                NodeId::new(),
                now + Duration::seconds(2),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let applying = repository
        .mark_dispatched(
            first_deployment_id,
            scheduled.aggregate_version,
            command.value.id,
            now + Duration::seconds(3),
        )
        .await?;
    let verifying = repository
        .mark_verifying(
            first_deployment_id,
            applying.aggregate_version,
            now + Duration::seconds(4),
        )
        .await?;
    let (active_workload, active) = repository
        .activate(
            first_deployment_id,
            verifying.aggregate_version,
            false,
            now + Duration::seconds(5),
        )
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(active_workload.active_revision_id, Some(first_revision_id));
    assert_eq!(
        repository
            .activate(
                first_deployment_id,
                verifying.aggregate_version,
                false,
                now + Duration::seconds(5),
            )
            .await?,
        (active_workload.clone(), active.clone())
    );

    let second_request = request(
        active_workload.clone(),
        2,
        'b',
        "deploy-http-fixture-v2",
        now + Duration::seconds(6),
    )?;
    let second_id = second_request.deployment.id;
    repository.create_deployment(second_request).await?;
    let second = repository
        .mark_resolving(second_id, 1, now + Duration::seconds(7))
        .await?;
    let failed = repository
        .fail(
            second_id,
            second.aggregate_version,
            "health check never stabilized".into(),
            now + Duration::seconds(8),
        )
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert_eq!(
        repository
            .find_workload(organization_id, active_workload.id)
            .await?
            .active_revision_id,
        Some(first_revision_id)
    );

    let third_request = request(
        active_workload.clone(),
        3,
        'c',
        "deploy-http-fixture-v3",
        now + Duration::seconds(9),
    )?;
    let third_id = third_request.deployment.id;
    repository.create_deployment(third_request).await?;
    assert_eq!(
        repository
            .cancel(third_id, 1, now + Duration::seconds(10))
            .await?
            .status,
        DeploymentStatus::Cancelled
    );
    assert_eq!(
        repository
            .list_deployments(organization_id, active_workload.id)
            .await?
            .len(),
        3
    );

    let rolled_back_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Atomic rollback fixture")?,
        now + Duration::seconds(11),
    );
    let mut rolled_back = request(
        rolled_back_workload,
        1,
        'd',
        "deploy-atomic-rollback",
        now + Duration::seconds(11),
    )?;
    let rolled_back_workload_id = rolled_back.workload.id;
    let rolled_back_operation_id = rolled_back.operation.id;
    rolled_back.event.schema_version = 0;
    assert!(matches!(
        repository.create_deployment(rolled_back).await,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workloads where id = ")
                    .bind(rolled_back_workload_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from operation_requests where operation_id = ")
                    .bind(rolled_back_operation_id.as_uuid()),
            )
            .await?,
        0
    );
    let candidate = request(
        active_workload.clone(),
        4,
        'e',
        "deploy-http-fixture-v4",
        now + Duration::seconds(12),
    )?;
    let candidate_revision_id = candidate.revision.id;
    let candidate_deployment_id = candidate.deployment.id;
    repository.create_deployment(candidate).await?;
    let advanced_replica = repository
        .find_workload_replica(organization_id, active_workload.id, replica_id)
        .await?;
    assert_eq!(advanced_replica.id, replica_id);
    assert_eq!(advanced_replica.generation, 4);
    assert_eq!(advanced_replica.revision_id, candidate_revision_id);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from deployment_replica_bindings where replica_id = ",
                )
                .bind(replica_id.as_uuid()),
            )
            .await?,
        4
    );
    Ok(WorkloadFixture {
        workload_id: active_workload.id,
        deployment_id: first_deployment_id,
        revision_id: first_revision_id,
        revision_generation: 1,
        candidate_revision_id,
        candidate_generation: 4,
        candidate_deployment_id,
        node_id,
    })
}

pub async fn exercise_replica_set(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::from_uuid(project_uuid),
        EnvironmentId::from_uuid(environment_uuid),
        ResourceName::parse("PostgreSQL replica set")?,
        Utc::now(),
    );
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let mut bundle = request(
        workload.clone(),
        1,
        'd',
        "postgres-replica-set",
        workload.created_at,
    )?;
    bundle.control = WorkloadControlSpec::unmanaged_replica_set(1, 3)?;
    let replay = bundle.clone();
    repository.create_deployment(bundle).await?;
    assert!(repository.create_deployment(replay).await?.replayed);

    let replicas = repository
        .list_workload_replicas(organization_id, workload.id)
        .await?;
    assert_eq!(
        replicas
            .iter()
            .map(|replica| replica.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(replicas.iter().all(|replica| {
        replica.lifecycle == WorkloadReplicaLifecycle::Desired
            && replica.revision_generation == 1
            && replica.generation == 1
    }));
    assert_eq!(replicas[0].id.as_uuid(), workload.id.as_uuid());
    assert_ne!(replicas[1].id, replicas[2].id);
    for replica in &replicas {
        let members = repository
            .list_workload_replica_members(organization_id, replica.id)
            .await?;
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id.as_uuid(), replica.id.as_uuid());
        assert_eq!(members[0].node_id, None);
    }
    let stored_count = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            sql_query::<i64>("select count(*) from workload_replicas where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and workload_id = ")
                .bind(workload.id.as_uuid()),
        )
        .await?;
    assert_eq!(stored_count, 3);
    Ok(())
}

pub(crate) fn request(
    workload: Workload,
    generation: u64,
    digest_character: char,
    idempotency_key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        generation,
        template(digest_character),
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
        WorkflowIdentity::new("cloud.deployment", "2")?,
        json!({
            "deploymentId": deployment.id,
            "generation": generation,
            "revisionId": revision.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    let canonical = serde_json::to_vec(&json!({
        "workloadId": workload.id,
        "generation": generation,
        "templateDigest": revision.template_digest,
    }))?;
    Ok(CreateDeploymentBundle {
        workload,
        control: a3s_cloud_control_plane::modules::workloads::WorkloadControlSpec::unmanaged_single_replica(),
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new("workload.deploy", idempotency_key, &canonical)?,
        event,
    })
}

fn template(digest_character: char) -> ServiceTemplate {
    let digest = digest(digest_character);
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/a3s-cloud/http-fixture@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/http-fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 250,
            memory_bytes: 64 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        }),
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
