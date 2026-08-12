use a3s_cloud_contracts::NodeCommandPayload;
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeDrainRepository, INodeRepository,
};
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, NodePoolId,
    OperationId, OrganizationId, ProjectId, RepositoryError, ResourceName, WorkloadId,
    WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_control_plane::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentReplicaBinding, DeploymentRequested,
    DeploymentStatus, HttpHealthCheck, IWorkloadReplicaDeploymentRepository,
    IWorkloadReplicaEvacuationRepository, IWorkloadReplicaRetirementRepository,
    IWorkloadRepository, IWorkloadRuntimeTargetRepository, OciArtifact, PostgresWorkloadRepository,
    ReconfigureReplicaSetWrite, ReplicaAntiAffinity, ReplicaEvacuationRequest,
    ReplicaRetirementCompletion, ReplicaRetirementDispatch, ReplicaRuntimeFence, ServicePort,
    ServiceProcess, ServiceResources, ServiceTemplate, Workload, WorkloadControl,
    WorkloadControlSpec, WorkloadReplicaLifecycle, WorkloadRevision,
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

pub struct ReplicaSetFixture {
    pub bindings: Vec<DeploymentReplicaBinding>,
}

pub async fn exercise_workload_node_pool_selection(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
    node_pool_id: NodePoolId,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let project_id = ProjectId::from_uuid(project_uuid);
    let environment_id = EnvironmentId::from_uuid(environment_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Pool-selected fixture")?,
        now,
    );
    let mut selected = request(workload, 1, 'e', "pool-selected-fixture", now)?;
    selected.control = WorkloadControlSpec::unmanaged_single_replica_in_pool(node_pool_id)?;
    let workload_id = selected.workload.id;
    repository.create_deployment(selected).await?;

    let control = repository
        .find_workload_control(organization_id, workload_id)
        .await?;
    assert_eq!(
        control.spec.placement_policy.node_pool_id(),
        Some(node_pool_id)
    );
    assert_eq!(
        Database::new(PostgresDialect, executor.clone())
            .fetch_one_as(
                sql_query::<Option<Uuid>>(
                    "select node_pool_id from workload_controls where workload_id = ",
                )
                .bind(workload_id.as_uuid()),
            )
            .await?,
        Some(node_pool_id.as_uuid())
    );

    let invalid_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Missing-pool fixture")?,
        now + Duration::milliseconds(1),
    );
    let mut invalid = request(
        invalid_workload,
        1,
        'f',
        "missing-pool-fixture",
        now + Duration::milliseconds(1),
    )?;
    invalid.control = WorkloadControlSpec::unmanaged_single_replica_in_pool(NodePoolId::new())?;
    assert!(matches!(
        repository.create_deployment(invalid).await,
        Err(RepositoryError::Storage(_))
    ));
    Ok(())
}

pub async fn exercise_replica_policy_v1_upgrade(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    project_uuid: Uuid,
    environment_uuid: Uuid,
    replica_set: &ReplicaSetFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let project_id = ProjectId::from_uuid(project_uuid);
    let environment_id = EnvironmentId::from_uuid(environment_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let workloads = repository
        .list_workloads(organization_id, project_id, environment_id)
        .await?;
    let mut before = BTreeMap::new();
    for workload in &workloads {
        let control = repository
            .find_workload_control(organization_id, workload.id)
            .await?;
        before.insert(
            workload.id,
            (
                control.spec.placement_policy.generation(),
                control.spec.placement_policy.desired_replicas(),
                control.aggregate_version,
                control.updated_at,
            ),
        );
    }
    if before.is_empty() {
        return Err("replica policy migration fixture has no Workload controls".into());
    }

    let client = executor.pool().get().await?;
    client
        .batch_execute(
            r#"
alter table workload_controls
    drop constraint workload_controls_placement_policy_check;

drop index workload_controls_node_pool_idx;

alter table workload_controls
    drop constraint workload_controls_node_pool_fk,
    drop column node_pool_id;

with policy_values as (
    select
        workload_id,
        (placement_policy ->> 'generation')::bigint as generation,
        (placement_policy ->> 'desiredReplicas')::integer as desired_replicas,
        (placement_policy ->> 'membersPerReplica')::integer as members_per_replica,
        placement_policy ->> 'topology' as topology
    from workload_controls
),
legacy as (
    select
        workload_id,
        generation,
        desired_replicas,
        members_per_replica,
        topology,
        'sha256:' || encode(
            sha256(convert_to(
                '{"schema":"a3s.cloud.effective-placement-policy.v1","generation":'
                    || generation::text
                    || ',"desiredReplicas":' || desired_replicas::text
                    || ',"membersPerReplica":' || members_per_replica::text
                    || ',"topology":"' || topology || '"}',
                'UTF8'
            )),
            'hex'
        ) as digest
    from policy_values
)
update workload_controls as control
set placement_policy = jsonb_build_object(
        'schema', 'a3s.cloud.effective-placement-policy.v1',
        'generation', legacy.generation,
        'desiredReplicas', legacy.desired_replicas,
        'membersPerReplica', legacy.members_per_replica,
        'topology', legacy.topology,
        'digest', legacy.digest
    ),
    placement_policy_digest = legacy.digest
from legacy
where legacy.workload_id = control.workload_id;

alter table workload_controls
    add constraint workload_controls_placement_policy_check check (
        jsonb_typeof(placement_policy) = 'object'
        and placement_policy ->> 'schema' =
            'a3s.cloud.effective-placement-policy.v1'
        and (placement_policy ->> 'generation')::bigint > 0
        and (placement_policy ->> 'desiredReplicas')::integer between 0 and 100
        and (placement_policy ->> 'membersPerReplica')::integer = 1
        and placement_policy ->> 'topology' = 'single_node'
        and placement_policy ->> 'digest' = placement_policy_digest
        and placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    );

drop index resource_claims_active_workload_node_replica_idx;
"#,
        )
        .await?;
    let node_id = PostgresNodeRepository::new(executor.clone())
        .list(organization_id)
        .await?
        .into_iter()
        .next()
        .ok_or("replica policy migration fixture has no node")?
        .id;
    let mut conflicting_claim_ids = Vec::new();
    for binding in replica_set.bindings.iter().take(2) {
        let claim_id = Uuid::now_v7();
        let replica_generation = i64::try_from(binding.replica_generation)?;
        let runtime_generation = i64::try_from(binding.runtime_generation)?;
        let at = Utc::now().max(binding.updated_at);
        client
            .execute(
                "insert into resource_claims (
                    id, organization_id, project_id, environment_id, workload_id,
                    deployment_id, replica_id, replica_generation, member_id,
                    placement_generation, node_id, inventory_generation, inventory_digest,
                    runtime_unit_id, runtime_generation, topology_digest, reservation_digest,
                    claim_generation, claim_digest, state, aggregate_version, created_at, updated_at
                 ) values (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, 1, $10, 1, $11,
                    $12, $13, $14, $15, 1, $16, 'reserved_in_db', 1, $17, $17
                 )",
                &[
                    &claim_id,
                    &binding.organization_id.as_uuid(),
                    &binding.project_id.as_uuid(),
                    &binding.environment_id.as_uuid(),
                    &binding.workload_id.as_uuid(),
                    &binding.deployment_id.as_uuid(),
                    &binding.replica_id.as_uuid(),
                    &replica_generation,
                    &binding.member_id.as_uuid(),
                    &node_id.as_uuid(),
                    &format!("sha256:{}", "a".repeat(64)),
                    &binding.runtime_unit_id,
                    &runtime_generation,
                    &format!("sha256:{}", "b".repeat(64)),
                    &format!("sha256:{}", "c".repeat(64)),
                    &format!("sha256:{}", "d".repeat(64)),
                    &at,
                ],
            )
            .await?;
        conflicting_claim_ids.push(claim_id);
    }
    if conflicting_claim_ids.len() != 2 {
        return Err("replica policy migration fixture omitted conflicting replicas".into());
    }
    let migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/088_required_replica_anti_affinity.sql"
    ));
    let conflict = client
        .batch_execute(migration)
        .await
        .expect_err("migration must reject an existing sibling placement conflict");
    if !conflict.as_db_error().is_some_and(|error| {
        error
            .message()
            .contains("required Workload replica anti-affinity")
    }) {
        return Err(format!("migration rejected the conflict unexpectedly: {conflict}").into());
    }
    for claim_id in conflicting_claim_ids {
        client
            .execute("delete from resource_claims where id = $1", &[&claim_id])
            .await?;
    }
    client.batch_execute(migration).await?;
    let node_pool_migration = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/092_workload_node_pool_selection.sql"
    ));
    client.batch_execute(node_pool_migration).await?;

    for (workload_id, (generation, desired_replicas, aggregate_version, updated_at)) in before {
        let control = repository
            .find_workload_control(organization_id, workload_id)
            .await?;
        assert_eq!(
            (
                control.spec.placement_policy.generation(),
                control.spec.placement_policy.desired_replicas(),
            ),
            (
                generation
                    .checked_add(2)
                    .ok_or("placement policy generation overflowed in migration fixture")?,
                desired_replicas,
            )
        );
        assert_eq!(control.aggregate_version, aggregate_version + 2);
        assert!(control.updated_at >= updated_at);
        assert_eq!(
            control.spec.placement_policy.replica_anti_affinity(),
            ReplicaAntiAffinity::Required
        );
        assert_eq!(
            control.spec.placement_policy.schema(),
            "a3s.cloud.effective-placement-policy.v3"
        );
        assert_eq!(control.spec.placement_policy.node_pool_id(), None);
    }
    Ok(())
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
) -> Result<ReplicaSetFixture, Box<dyn std::error::Error>> {
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
    let database = Database::new(PostgresDialect, executor.clone());
    let stored_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from workload_replicas where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and workload_id = ")
                .bind(workload.id.as_uuid()),
        )
        .await?;
    assert_eq!(stored_count, 3);
    let candidates = repository.pending_replica_deployments(10).await?;
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.replica_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let first_candidate = candidates[0];
    let (left, right) = tokio::join!(
        repository.materialize_replica_deployment(first_candidate, workload.created_at),
        repository.materialize_replica_deployment(first_candidate, workload.created_at),
    );
    let left = left?.ok_or("left PostgreSQL replica materialization was skipped")?;
    let right = right?.ok_or("right PostgreSQL replica materialization was skipped")?;
    assert_ne!(left.created, right.created);
    assert_eq!(left.deployment, right.deployment);
    assert_eq!(left.operation, right.operation);
    let remaining = repository.pending_replica_deployments(10).await?;
    assert_eq!(remaining.len(), 1);
    assert!(repository
        .materialize_replica_deployment(remaining[0], workload.created_at)
        .await?
        .is_some_and(|result| result.created));
    assert!(repository.pending_replica_deployments(10).await?.is_empty());
    let mut replica_bindings = Vec::new();
    for deployment in repository
        .list_deployments(organization_id, workload.id)
        .await?
    {
        replica_bindings.push(
            repository
                .find_deployment_replica_binding(organization_id, deployment.id)
                .await?,
        );
    }
    replica_bindings.sort_by_key(|binding| binding.replica_id);
    assert_eq!(replica_bindings.len(), 3);
    assert!(replica_bindings
        .iter()
        .all(|binding| binding.node_id.is_none()));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64, i64, i64)>(
                    "select count(*), count(distinct binding.runtime_unit_id), count(distinct deployment.operation_id), count(*) filter (where deployment.status = 'queued') from deployment_replica_bindings binding join deployments deployment on deployment.id = binding.deployment_id where binding.organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and binding.workload_id = ")
                .bind(workload.id.as_uuid()),
            )
            .await?,
        (3, 3, 3, 3)
    );

    let scalable_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::from_uuid(project_uuid),
        EnvironmentId::from_uuid(environment_uuid),
        ResourceName::parse("PostgreSQL scalable replica set")?,
        Utc::now(),
    );
    repository
        .create_deployment(request(
            scalable_workload.clone(),
            1,
            'e',
            "postgres-scalable-replica-set",
            scalable_workload.created_at,
        )?)
        .await?;
    let initial_control = repository
        .find_workload_control(organization_id, scalable_workload.id)
        .await?;
    let left_write = replica_set_write(
        &initial_control,
        3,
        "postgres-replica-set-scale-up-left",
        scalable_workload.created_at + Duration::seconds(1),
    )?;
    let right_write = replica_set_write(
        &initial_control,
        3,
        "postgres-replica-set-scale-up-right",
        scalable_workload.created_at + Duration::seconds(1),
    )?;
    let (left, right) = tokio::join!(
        repository.reconfigure_replica_set(left_write.clone()),
        repository.reconfigure_replica_set(right_write.clone())
    );
    let (winner, winning_write, loser) = match (left, right) {
        (Ok(winner), Err(loser)) => (winner, left_write, loser),
        (Err(loser), Ok(winner)) => (winner, right_write, loser),
        outcomes => {
            return Err(
                format!("expected one PostgreSQL replica-set writer, got {outcomes:?}").into(),
            )
        }
    };
    assert!(matches!(loser, RepositoryError::Conflict(_)));
    assert!(!winner.replayed);
    assert_eq!(winner.control.aggregate_version, 2);
    assert_eq!(winner.control.spec.placement_policy.generation(), 2);
    assert_eq!(winner.control.spec.placement_policy.desired_replicas(), 3);
    assert_eq!(
        winner
            .replicas
            .iter()
            .map(|replica| replica.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(
        repository
            .reconfigure_replica_set(winning_write.clone())
            .await?
            .replayed
    );
    let conflicting_replay = ReconfigureReplicaSetWrite {
        desired_replicas: 2,
        idempotency: IdempotencyRequest::new(
            winning_write.idempotency.scope.clone(),
            winning_write.idempotency.key.clone(),
            b"different PostgreSQL replica-set request",
        )?,
        ..winning_write
    };
    assert!(matches!(
        repository.reconfigure_replica_set(conflicting_replay).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let scalable_candidates = repository
        .pending_replica_deployments(10)
        .await?
        .into_iter()
        .filter(|candidate| candidate.workload_id == scalable_workload.id)
        .collect::<Vec<_>>();
    assert_eq!(scalable_candidates.len(), 2);
    let queued_materialization = repository
        .materialize_replica_deployment(
            scalable_candidates[0],
            scalable_workload.created_at + Duration::seconds(1),
        )
        .await?
        .ok_or("queued replica deployment materialization")?;
    let stale_materialization = scalable_candidates[1];

    let scaled_down = repository
        .reconfigure_replica_set(replica_set_write(
            &winner.control,
            1,
            "postgres-replica-set-scale-down",
            scalable_workload.created_at + Duration::seconds(2),
        )?)
        .await?;
    assert_eq!(scaled_down.control.aggregate_version, 3);
    assert_eq!(scaled_down.control.spec.placement_policy.generation(), 3);
    assert_eq!(
        scaled_down
            .replicas
            .iter()
            .map(|replica| replica.lifecycle)
            .collect::<Vec<_>>(),
        vec![
            WorkloadReplicaLifecycle::Desired,
            WorkloadReplicaLifecycle::Retiring,
            WorkloadReplicaLifecycle::Retiring,
        ]
    );
    let persisted = repository
        .list_workload_replicas(organization_id, scalable_workload.id)
        .await?;
    assert_eq!(persisted, scaled_down.replicas);
    assert!(matches!(
        repository
            .mark_resolving(
                queued_materialization.deployment.id,
                queued_materialization.deployment.aggregate_version,
                scalable_workload.created_at + Duration::seconds(3),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(
        repository
            .materialize_replica_deployment(
                stale_materialization,
                scalable_workload.created_at + Duration::seconds(3),
            )
            .await?
            .is_none(),
        "a candidate retired before its lock was acquired must not create a deployment",
    );
    let retiring_targets = repository.pending_replica_retirements(10).await?;
    assert_eq!(retiring_targets.len(), 2);
    for target in retiring_targets {
        assert_eq!(target.replica.workload_id, scalable_workload.id);
        assert_eq!(target.replica.lifecycle, WorkloadReplicaLifecycle::Retiring);
        assert_eq!(target.member.node_id, None);
        assert_eq!(target.replica.retirement_command_id, None);
        assert_eq!(target.replica.runtime_fenced_at, None);
        let completion = ReplicaRetirementCompletion {
            organization_id,
            workload_id: scalable_workload.id,
            replica_id: target.replica.id,
            replica_generation: target.replica.generation,
            expected_replica_version: target.replica.aggregate_version,
            member_id: target.member.id,
            expected_member_version: target.member.aggregate_version,
            fenced_node_id: None,
            completed_at: scalable_workload.created_at + Duration::seconds(4),
            correlation_id: Uuid::now_v7(),
        };
        let completed = repository.complete_replica_retirement(completion).await?;
        assert!(!completed.replayed);
        assert_eq!(completed.value.lifecycle, WorkloadReplicaLifecycle::Retired);
        assert!(
            repository
                .complete_replica_retirement(completion)
                .await?
                .replayed
        );
    }
    assert!(repository.pending_replica_retirements(10).await?.is_empty());
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'workload.replica.retired'",
            ))
            .await?,
        2
    );
    let retired = repository
        .list_workload_replicas(organization_id, scalable_workload.id)
        .await?;
    assert_eq!(
        retired
            .iter()
            .map(|replica| replica.lifecycle)
            .collect::<Vec<_>>(),
        vec![
            WorkloadReplicaLifecycle::Desired,
            WorkloadReplicaLifecycle::Retired,
            WorkloadReplicaLifecycle::Retired,
        ]
    );
    let retired_ids = retired.iter().map(|replica| replica.id).collect::<Vec<_>>();
    let retired_control = repository
        .find_workload_control(organization_id, scalable_workload.id)
        .await?;
    let reactivated = repository
        .reconfigure_replica_set(replica_set_write(
            &retired_control,
            3,
            "postgres-replica-set-reactivate",
            scalable_workload.created_at + Duration::seconds(5),
        )?)
        .await?;
    assert_eq!(
        reactivated
            .replicas
            .iter()
            .map(|replica| replica.id)
            .collect::<Vec<_>>(),
        retired_ids
    );
    assert!(reactivated.replicas.iter().all(|replica| {
        replica.lifecycle == WorkloadReplicaLifecycle::Desired
            && replica.retirement_command_id.is_none()
            && replica.runtime_fenced_at.is_none()
    }));
    assert_eq!(reactivated.replicas[1].generation, 2);
    assert_eq!(reactivated.replicas[2].generation, 2);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from workload_replica_members where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and workload_id = ")
                .bind(scalable_workload.id.as_uuid()),
            )
            .await?,
        3
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ",)
                    .bind(scalable_workload.id.as_uuid())
                    .append(" and event_key = 'workload.replica-set.reconfigured'"),
            )
            .await?,
        3
    );
    assert!(
        repository
            .list_active_runtime_targets(100)
            .await?
            .is_empty(),
        "queued replica generations must not enter active Runtime reconciliation",
    );
    Ok(ReplicaSetFixture {
        bindings: replica_bindings,
    })
}

pub async fn exercise_replica_evacuation(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
    replica_set: &mut ReplicaSetFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let repository = PostgresWorkloadRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let selected_index = replica_set
        .bindings
        .len()
        .checked_sub(1)
        .ok_or("replica evacuation fixture has no deployment binding")?;
    let source_binding = replica_set.bindings[selected_index].clone();
    let source_deployment = repository
        .find_deployment(organization_id, source_binding.deployment_id)
        .await?;
    let (source_node_uuid, previous_node_state) = database
        .fetch_one_as(
            sql_query::<(Uuid, String)>(
                "select nodes.id, nodes.state from nodes join node_resource_inventory_heads inventory on inventory.organization_id = nodes.organization_id and inventory.node_id = nodes.id where nodes.organization_id = ",
            )
            .bind(organization_uuid)
            .append(" order by nodes.id asc limit 1"),
        )
        .await?;
    let source_node_id = NodeId::from_uuid(source_node_uuid);
    database
        .execute(
            sql_query::<()>("update nodes set state = 'ready' where organization_id = ")
                .bind(organization_uuid)
                .append(" and id = ")
                .bind(source_node_uuid),
        )
        .await?;
    let base = Utc::now()
        .max(source_binding.updated_at + Duration::seconds(1))
        .max(source_deployment.updated_at + Duration::seconds(1));
    let resolving = repository
        .mark_resolving(
            source_deployment.id,
            source_deployment.aggregate_version,
            base,
        )
        .await?;
    let placed = repository
        .assign_node(
            source_deployment.id,
            resolving.aggregate_version,
            source_node_id,
            base + Duration::seconds(1),
        )
        .await?;
    let placed_member = repository
        .find_workload_replica_member(
            organization_id,
            source_binding.replica_id,
            source_binding.member_id,
        )
        .await?;
    assert_eq!(placed_member.node_id, Some(source_node_id));
    assert_eq!(placed_member.placement_generation, 1);

    database
        .execute(
            sql_query::<()>("update nodes set state = 'draining' where organization_id = ")
                .bind(organization_uuid)
                .append(" and id = ")
                .bind(source_node_uuid),
        )
        .await?;
    let node_repository = PostgresNodeRepository::new(executor.clone());
    assert!(node_repository
        .list_evacuation_sources(Utc::now(), 100)
        .await?
        .iter()
        .any(|source| source.node.id == source_node_id));

    let candidates = repository
        .pending_replica_evacuations(organization_id, source_node_id, 100)
        .await?
        .into_iter()
        .filter(|candidate| candidate.replica_id == source_binding.replica_id)
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1);
    let candidate = candidates[0];
    assert_eq!(
        candidate.replica_generation,
        source_binding.replica_generation
    );
    assert_eq!(candidate.member_id, source_binding.member_id);
    assert_eq!(candidate.placement_generation, 1);
    let request = ReplicaEvacuationRequest {
        candidate,
        requested_at: base + Duration::seconds(2),
        correlation_id: Uuid::now_v7(),
    };
    let (left, right) = tokio::join!(
        repository.request_replica_evacuation(request),
        repository.request_replica_evacuation(request),
    );
    let left = left?;
    let right = right?;
    assert_ne!(left.replayed, right.replayed);
    assert_eq!(left.value, right.value);
    let requested = left.value;
    assert_eq!(requested.lifecycle, WorkloadReplicaLifecycle::Retiring);
    assert_eq!(requested.evacuation_node_id, Some(source_node_id));
    assert!(repository
        .pending_replica_evacuations(organization_id, source_node_id, 100)
        .await?
        .into_iter()
        .all(|candidate| candidate.replica_id != source_binding.replica_id));
    assert!(matches!(
        repository
            .mark_dispatched(
                source_deployment.id,
                placed.aggregate_version,
                NodeCommandId::new(),
                base + Duration::seconds(3),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let target = repository
        .pending_replica_retirements(100)
        .await?
        .into_iter()
        .find(|target| target.replica.id == source_binding.replica_id)
        .ok_or("requested replica evacuation was not exposed to retirement")?;
    let command_id = NodeCommandId::new();
    let dispatched = repository
        .dispatch_replica_retirement(ReplicaRetirementDispatch {
            organization_id,
            workload_id: source_binding.workload_id,
            replica_id: source_binding.replica_id,
            replica_generation: source_binding.replica_generation,
            expected_replica_version: target.replica.aggregate_version,
            command_id,
            dispatched_at: base + Duration::seconds(3),
        })
        .await?;
    let fenced = repository
        .record_replica_runtime_fenced(ReplicaRuntimeFence {
            organization_id,
            workload_id: source_binding.workload_id,
            replica_id: source_binding.replica_id,
            replica_generation: source_binding.replica_generation,
            expected_replica_version: dispatched.aggregate_version,
            command_id,
            fenced_at: base + Duration::seconds(4),
        })
        .await?;
    let completion = ReplicaRetirementCompletion {
        organization_id,
        workload_id: source_binding.workload_id,
        replica_id: source_binding.replica_id,
        replica_generation: source_binding.replica_generation,
        expected_replica_version: fenced.aggregate_version,
        member_id: source_binding.member_id,
        expected_member_version: target.member.aggregate_version,
        fenced_node_id: Some(source_node_id),
        completed_at: base + Duration::seconds(5),
        correlation_id: Uuid::now_v7(),
    };
    let completed = repository.complete_replica_retirement(completion).await?;
    assert!(!completed.replayed);
    assert_eq!(completed.value.id, source_binding.replica_id);
    assert_eq!(
        completed.value.generation,
        source_binding.replica_generation + 1
    );
    assert_eq!(completed.value.lifecycle, WorkloadReplicaLifecycle::Desired);
    assert_eq!(completed.value.evacuation_node_id, None);
    assert!(
        repository
            .complete_replica_retirement(completion)
            .await?
            .replayed
    );
    let released_member = repository
        .find_workload_replica_member(
            organization_id,
            source_binding.replica_id,
            source_binding.member_id,
        )
        .await?;
    assert_eq!(released_member.node_id, None);
    assert_eq!(released_member.placement_generation, 1);

    let replacement_candidate = repository
        .pending_replica_deployments(100)
        .await?
        .into_iter()
        .find(|candidate| candidate.replica_id == source_binding.replica_id)
        .ok_or("evacuated replica generation was not rematerialized")?;
    assert_eq!(
        replacement_candidate.replica_generation,
        completed.value.generation
    );
    let replacement = repository
        .materialize_replica_deployment(replacement_candidate, base + Duration::seconds(6))
        .await?
        .ok_or("evacuated replica replacement materialization was skipped")?;
    assert!(replacement.created);
    assert_ne!(replacement.deployment.id, source_deployment.id);
    let replacement_binding = repository
        .find_deployment_replica_binding(organization_id, replacement.deployment.id)
        .await?;
    assert_eq!(replacement_binding.replica_id, source_binding.replica_id);
    assert_eq!(
        replacement_binding.replica_generation,
        completed.value.generation
    );
    assert_eq!(replacement_binding.node_id, None);
    assert_eq!(replacement_binding.placement_generation, 1);
    replica_set.bindings[selected_index] = replacement_binding;

    for event_key in [
        "workload.replica.evacuation.requested",
        "workload.replica.evacuated",
    ] {
        assert_eq!(
            database
                .fetch_one_as(
                    sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ",)
                        .bind(source_binding.replica_id.as_uuid())
                        .append(" and event_key = ")
                        .bind(event_key),
                )
                .await?,
            1
        );
    }
    database
        .execute(
            sql_query::<()>("update nodes set state = ")
                .bind(previous_node_state)
                .append(" where organization_id = ")
                .bind(organization_uuid)
                .append(" and id = ")
                .bind(source_node_uuid),
        )
        .await?;
    Ok(())
}

fn replica_set_write(
    control: &WorkloadControl,
    desired_replicas: u32,
    idempotency_key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> Result<ReconfigureReplicaSetWrite, Box<dyn std::error::Error>> {
    let canonical = serde_json::to_vec(&json!({
        "organizationId": control.organization_id,
        "workloadId": control.workload_id,
        "expectedPolicyGeneration": control.spec.placement_policy.generation(),
        "desiredReplicas": desired_replicas,
    }))?;
    Ok(ReconfigureReplicaSetWrite {
        organization_id: control.organization_id,
        workload_id: control.workload_id,
        expected_control_version: control.aggregate_version,
        expected_policy_generation: control.spec.placement_policy.generation(),
        desired_replicas,
        managed_owner: control.spec.managed_owner.clone(),
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/workloads/{}/replica-set",
                control.organization_id, control.workload_id
            ),
            idempotency_key,
            &canonical,
        )?,
        correlation_id: Uuid::now_v7(),
        requested_at,
    })
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
