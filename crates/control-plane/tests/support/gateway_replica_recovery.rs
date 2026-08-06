use crate::gateway_rollouts_support::GatewayRolloutFixture;
use a3s_cloud_contracts::{
    AppliedGatewaySnapshot, GatewayAckState, GatewayManagementProtocol, GatewaySnapshot,
    GatewaySnapshotObservationState, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandResult, NodeGatewayAck, NodeGatewaySnapshotObservation,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    GatewayRolloutStaged, GatewayScopeCreated,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, GatewayReplicaRecoveryTarget, IEdgeRepository, StageGatewayRollout,
};
use a3s_cloud_control_plane::modules::edge::{
    FleetGatewayObservationQueue, GatewayPublication, GatewayReplicaRecovery,
    GatewayReplicaRecoveryReconciler, GatewayReplicaRecoveryState, GatewayReplicaRolloutState,
    GatewayRollout, GatewayRolloutPolicy, GatewayRolloutRollbackState, GatewayScope,
    IGatewayObservationQueue, PostgresEdgeRepository,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub async fn exercise_gateway_replica_recovery(
    executor: &PostgresExecutor,
    fixture: GatewayRolloutFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresEdgeRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let now = Utc::now();
    let nodes = (0..4).map(|_| NodeId::new()).collect::<Vec<_>>();
    let mut agents = BTreeMap::new();
    for (ordinal, node_id) in nodes.iter().enumerate() {
        let agent_instance_id = Uuid::now_v7();
        agents.insert(*node_id, agent_instance_id);
        database
            .execute(
                sql_query::<()>(
                    "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, aggregate_version) values (",
                )
                .bind(fixture.organization_id.as_uuid())
                .append(", ")
                .bind(node_id.as_uuid())
                .append(", ")
                .bind(format!("Gateway recovery fixture {}", ordinal + 1))
                .append(", ")
                .bind(format!("gateway-recovery-fixture-{node_id}"))
                .append(", 'ready', ")
                .bind(agent_instance_id)
                .append(", 'test', 'test-runtime', 'gateway-recovery-test', ")
                .bind(format!("sha256:{}", "a".repeat(64)))
                .append(", ")
                .bind(serde_json::json!({}))
                .append(", ")
                .bind(now)
                .append(", ")
                .bind(now)
                .append(", 1)"),
            )
            .await?;
    }

    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        nodes[0],
        nodes.clone(),
        GatewayRolloutPolicy::new(1, u32::try_from(nodes.len())? - 1, nodes.len())?,
        now,
    )?;
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "postgres-gateway-recovery-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;

    let prior_publications = publications(&nodes, 1, None, now)?;
    let prior = stage_rollout(
        &repository,
        &scope,
        1,
        prior_publications,
        "postgres-gateway-recovery-prior",
    )
    .await?;
    for publication in &prior.publications {
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(publication, now + Duration::seconds(1)),
                now + Duration::seconds(1) + Duration::microseconds(1),
            )
            .await?;
    }

    let candidate_issued_at = now + Duration::seconds(2);
    let candidate_publications = publications(&nodes, 2, Some(1), candidate_issued_at)?;
    let candidate = stage_rollout(
        &repository,
        &scope,
        2,
        candidate_publications.clone(),
        "postgres-gateway-recovery-candidate",
    )
    .await?;
    let unavailable_at = candidate_publications[0].command_not_after + Duration::seconds(1);
    let failure = "Gateway command expired before exact acknowledgement";
    let mut rollout = candidate.rollout;
    for node_id in &nodes {
        let expected_version = rollout.aggregate_version;
        rollout = repository
            .mark_gateway_rollout_replica_unavailable(
                fixture.organization_id,
                rollout.id,
                *node_id,
                expected_version,
                failure,
                unavailable_at,
            )
            .await?;
        assert_eq!(rollout.aggregate_version, expected_version + 1);
    }
    assert!(rollout.state.terminal());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_rollout_replicas where gateway_rollout_id = ",
                )
                .bind(rollout.id.as_uuid())
                .append(" and state = 'unavailable' and recovery is not null"),
            )
            .await?,
        i64::try_from(nodes.len())?
    );

    let restarted = PostgresEdgeRepository::new(executor.clone());
    assert_eq!(
        restarted
            .find_gateway_rollout_rollback(fixture.organization_id, rollout.id)
            .await?
            .state,
        GatewayRolloutRollbackState::Required
    );
    assert!(restarted
        .pending_gateway_rollout_rollbacks(100)
        .await?
        .into_iter()
        .all(|target| target.failed_rollout.id != rollout.id));
    let pending = restarted.pending_gateway_replica_recoveries(10).await?;
    let targets = pending
        .into_iter()
        .filter(|target| target.rollout.id == rollout.id)
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), nodes.len());
    for target in &targets {
        target.validate()?;
        assert_eq!(target.publication.revision, 2);
        assert_eq!(
            target
                .prior_publication
                .as_ref()
                .ok_or("PostgreSQL recovery omitted the prior publication")?
                .revision,
            1
        );
    }

    let worker_node = targets
        .first()
        .ok_or("PostgreSQL recovery scan returned no targets")?
        .publication
        .node_id;
    let remaining_nodes = nodes
        .iter()
        .copied()
        .filter(|node_id| *node_id != worker_node)
        .collect::<Vec<_>>();
    assert_eq!(remaining_nodes.len(), 3);
    let edge_port: Arc<dyn IEdgeRepository> =
        Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let node_control: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let observation_port: Arc<dyn IGatewayObservationQueue> =
        Arc::new(FleetGatewayObservationQueue::new(Arc::clone(&node_control)));
    let worker_at = unavailable_at + Duration::seconds(1);
    let worker = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&edge_port),
        Arc::clone(&observation_port),
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        1,
    )?;
    let staged_by_worker = worker.run_once(worker_at).await?;
    assert_eq!(staged_by_worker.recovery_targets, 1);
    assert_eq!(staged_by_worker.staged_attempts, 1);
    assert_eq!(staged_by_worker.dispatched_commands, 1);
    assert_eq!(staged_by_worker.pending_commands, 1);
    assert!(staged_by_worker.failures.is_empty());
    rollout = restarted
        .find_gateway_rollout(fixture.organization_id, rollout.id)
        .await?;
    let worker_command = recovery(&rollout, worker_node)?
        .command_id
        .ok_or("worker-staged recovery omitted its command ID")?;
    let lease = node_control
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: worker_node.as_uuid(),
                agent_instance_id: *agents
                    .get(&worker_node)
                    .ok_or("Gateway recovery fixture omitted its Agent identity")?,
                after_sequence: 0,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            worker_at + Duration::seconds(1),
            worker_at + Duration::seconds(30),
        )
        .await?;
    let envelope = lease
        .commands
        .first()
        .ok_or("Gateway recovery observation command was not leased")?;
    assert_eq!(envelope.command_id, worker_command.as_uuid());
    let worker_candidate = candidate_for(&targets, worker_node)?;
    let worker_observed_at = worker_at + Duration::seconds(2);
    let worker_observation = observation(
        worker_command,
        worker_candidate,
        GatewaySnapshotObservationState::Applied,
        Some(applied_snapshot(worker_candidate, worker_observed_at)),
        worker_observed_at,
    );
    node_control
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: envelope.command_id,
                lease_id: envelope.lease_id,
                node_id: envelope.node_id,
                sequence: envelope.sequence,
                payload_digest: envelope.payload_digest.clone(),
                completed_at: worker_observed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::GatewaySnapshotObserved {
                        observation: worker_observation,
                    }),
                },
            },
            worker_observed_at,
        )
        .await?;
    let restarted_worker = GatewayReplicaRecoveryReconciler::new(
        edge_port,
        observation_port,
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        1,
    )?;
    let projected_by_worker = restarted_worker
        .run_once(worker_observed_at + Duration::seconds(1))
        .await?;
    assert_eq!(projected_by_worker.observed_replicas, 1);
    assert_eq!(projected_by_worker.dispatched_commands, 0);
    assert!(projected_by_worker.failures.is_empty());
    rollout = restarted
        .find_gateway_rollout(fixture.organization_id, rollout.id)
        .await?;
    assert_eq!(
        recovery(&rollout, worker_node)?.state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(
        restarted
            .gateway_scope(worker_node)
            .await?
            .installed_revision,
        Some(worker_candidate.revision)
    );
    assert!(restarted
        .pending_gateway_rollout_rollbacks(100)
        .await?
        .into_iter()
        .all(|target| target.failed_rollout.id != rollout.id));

    let prior_node = remaining_nodes[0];
    let first_command = NodeCommandId::new();
    let first_issued_at = worker_observed_at + Duration::seconds(2);
    let stale_version = rollout.aggregate_version;
    rollout = stage_observation(
        &restarted,
        fixture,
        rollout,
        prior_node,
        first_command,
        first_issued_at,
    )
    .await?;
    let process_restart = PostgresEdgeRepository::new(executor.clone());
    let observing = recovery_target(&process_restart, rollout.id, prior_node).await?;
    assert_eq!(
        recovery(&observing.rollout, prior_node)?.state,
        GatewayReplicaRecoveryState::Observing
    );
    assert!(matches!(
        process_restart
            .record_gateway_replica_recovery_failure(
                fixture.organization_id,
                rollout.id,
                prior_node,
                stale_version,
                first_command,
                "stale writer must lose",
                true,
                first_issued_at + Duration::seconds(1),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let first_candidate = candidate_for(&targets, prior_node)?;
    rollout = record_observation(
        &process_restart,
        fixture,
        rollout,
        prior_node,
        observation(
            first_command,
            first_candidate,
            GatewaySnapshotObservationState::Applying,
            None,
            first_issued_at + Duration::seconds(1),
        ),
    )
    .await?;
    assert_eq!(
        recovery(&rollout, prior_node)?.state,
        GatewayReplicaRecoveryState::Required
    );
    let second_command = NodeCommandId::new();
    let second_issued_at = first_issued_at + Duration::seconds(2);
    rollout = stage_observation(
        &process_restart,
        fixture,
        rollout,
        prior_node,
        second_command,
        second_issued_at,
    )
    .await?;
    rollout = record_observation(
        &process_restart,
        fixture,
        rollout,
        prior_node,
        observation(
            second_command,
            first_candidate,
            GatewaySnapshotObservationState::NotApplied,
            Some(applied_snapshot(
                prior_for(&targets, prior_node)?,
                now + Duration::seconds(1),
            )),
            second_issued_at + Duration::seconds(1),
        ),
    )
    .await?;
    assert_eq!(
        recovery(&rollout, prior_node)?.state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery(&rollout, prior_node)?.attempt, 2);
    assert_eq!(
        process_restart
            .gateway_scope(prior_node)
            .await?
            .installed_revision,
        Some(prior_for(&targets, prior_node)?.revision)
    );

    let retry_node = remaining_nodes[1];
    let retry_command = NodeCommandId::new();
    let retry_at = first_issued_at + Duration::seconds(8);
    rollout = stage_observation(
        &process_restart,
        fixture,
        rollout,
        retry_node,
        retry_command,
        retry_at,
    )
    .await?;
    let expected_version = rollout.aggregate_version;
    rollout = process_restart
        .record_gateway_replica_recovery_failure(
            fixture.organization_id,
            rollout.id,
            retry_node,
            expected_version,
            retry_command,
            "Gateway observation command lease expired",
            true,
            retry_at + Duration::seconds(1),
        )
        .await?;
    assert_eq!(rollout.aggregate_version, expected_version + 1);
    assert_eq!(
        recovery(&rollout, retry_node)?.state,
        GatewayReplicaRecoveryState::Required
    );
    let uninitialized_command = NodeCommandId::new();
    let uninitialized_at = retry_at + Duration::seconds(2);
    rollout = stage_observation(
        &process_restart,
        fixture,
        rollout,
        retry_node,
        uninitialized_command,
        uninitialized_at,
    )
    .await?;
    rollout = record_observation(
        &process_restart,
        fixture,
        rollout,
        retry_node,
        observation(
            uninitialized_command,
            candidate_for(&targets, retry_node)?,
            GatewaySnapshotObservationState::Uninitialized,
            None,
            uninitialized_at + Duration::seconds(1),
        ),
    )
    .await?;
    assert_eq!(
        recovery(&rollout, retry_node)?.state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery(&rollout, retry_node)?.attempt, 2);
    assert_eq!(
        process_restart
            .gateway_scope(retry_node)
            .await?
            .installed_revision,
        None
    );

    let divergent_node = remaining_nodes[2];
    let divergent_command = NodeCommandId::new();
    let divergent_at = first_issued_at + Duration::seconds(13);
    rollout = stage_observation(
        &process_restart,
        fixture,
        rollout,
        divergent_node,
        divergent_command,
        divergent_at,
    )
    .await?;
    let divergent_candidate = candidate_for(&targets, divergent_node)?;
    let mut unknown = applied_snapshot(divergent_candidate, divergent_at);
    unknown.revision += 1;
    unknown.expected_revision = Some(divergent_candidate.revision);
    unknown.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    rollout = record_observation(
        &process_restart,
        fixture,
        rollout,
        divergent_node,
        observation(
            divergent_command,
            divergent_candidate,
            GatewaySnapshotObservationState::NotApplied,
            Some(unknown),
            divergent_at + Duration::seconds(1),
        ),
    )
    .await?;
    assert_eq!(
        recovery(&rollout, divergent_node)?.state,
        GatewayReplicaRecoveryState::Diverged
    );
    let diverged_rollback = process_restart
        .find_gateway_rollout_rollback(fixture.organization_id, rollout.id)
        .await?;
    assert_eq!(
        diverged_rollback.state,
        GatewayRolloutRollbackState::Diverged
    );
    assert_eq!(
        diverged_rollback.failure.as_deref(),
        Some("Gateway applied state does not match the candidate or its known prior publication")
    );
    assert_eq!(
        process_restart
            .gateway_scope(divergent_node)
            .await?
            .installed_revision,
        Some(prior_for(&targets, divergent_node)?.revision)
    );
    assert!(process_restart
        .pending_gateway_rollout_rollbacks(100)
        .await?
        .into_iter()
        .all(|target| target.failed_rollout.id != rollout.id));

    assert!(process_restart
        .pending_gateway_replica_recoveries(100)
        .await?
        .into_iter()
        .all(|target| target.rollout.id != rollout.id));
    assert_eq!(
        PostgresEdgeRepository::new(executor.clone())
            .find_gateway_rollout(fixture.organization_id, rollout.id)
            .await?,
        rollout
    );
    assert!(rollout.replicas.iter().all(|replica| {
        replica.state == GatewayReplicaRolloutState::Unavailable
            && replica.recovery.as_ref().is_some_and(|recovery| {
                matches!(
                    recovery.state,
                    GatewayReplicaRecoveryState::Observed | GatewayReplicaRecoveryState::Diverged
                )
            })
    }));
    Ok(())
}

async fn stage_rollout(
    repository: &PostgresEdgeRepository,
    scope: &GatewayScope,
    generation: u64,
    publications: Vec<GatewayPublication>,
    key: &str,
) -> Result<
    a3s_cloud_control_plane::modules::edge::domain::repositories::GatewayRolloutResult,
    Box<dyn std::error::Error>,
> {
    let rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        scope,
        generation,
        &publications,
        publications
            .first()
            .ok_or("Gateway recovery rollout omitted publications")?
            .command_issued_at,
    )?;
    let mut expected_scope_versions = BTreeMap::new();
    for node_id in &scope.member_node_ids {
        expected_scope_versions.insert(
            *node_id,
            repository.gateway_scope(*node_id).await?.aggregate_version,
        );
    }
    Ok(repository
        .stage_gateway_rollout(StageGatewayRollout {
            scope: scope.clone(),
            rollout: rollout.clone(),
            route_replicas: Vec::new(),
            publications,
            certificates: Vec::new(),
            expected_scope_versions,
            idempotency: IdempotencyRequest::new(
                format!("gateway-scopes/{}/recoveries", scope.id),
                key,
                rollout.id.to_string().as_bytes(),
            )?,
            event: GatewayRolloutStaged::envelope(scope, &rollout)?,
            route_event: None,
        })
        .await?)
}

fn publications(
    nodes: &[NodeId],
    revision: u64,
    expected_revision: Option<u64>,
    issued_at: DateTime<Utc>,
) -> Result<Vec<GatewayPublication>, String> {
    let correlation_id = Uuid::now_v7();
    nodes
        .iter()
        .map(|node_id| {
            let snapshot = GatewaySnapshot::new(
                node_id.as_uuid(),
                revision,
                expected_revision,
                issued_at,
                issued_at + Duration::hours(1),
                format!("# recovery snapshot {revision} for {node_id}"),
            )?;
            GatewayPublication::stage(
                *node_id,
                NodeCommandId::new(),
                correlation_id,
                snapshot,
                issued_at,
                issued_at + Duration::minutes(3),
            )
        })
        .collect()
}

fn acknowledgement(
    publication: &GatewayPublication,
    acknowledged_at: DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}

fn observation(
    command_id: NodeCommandId,
    candidate: &GatewayPublication,
    state: GatewaySnapshotObservationState,
    applied: Option<AppliedGatewaySnapshot>,
    observed_at: DateTime<Utc>,
) -> NodeGatewaySnapshotObservation {
    NodeGatewaySnapshotObservation {
        schema: NodeGatewaySnapshotObservation::SCHEMA.into(),
        observation_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: candidate.node_id.as_uuid(),
        gateway_id: candidate.node_id.as_uuid(),
        revision: candidate.revision,
        snapshot_digest: candidate.snapshot_digest.clone(),
        state,
        ready: state == GatewaySnapshotObservationState::Applied,
        applied,
        observed_at,
        management_protocol: GatewayManagementProtocol::advertised_v1(),
    }
}

fn applied_snapshot(
    publication: &GatewayPublication,
    applied_at: DateTime<Utc>,
) -> AppliedGatewaySnapshot {
    AppliedGatewaySnapshot {
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        expected_revision: publication.expected_revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        issued_at: publication.command_issued_at,
        expires_at: publication.snapshot_expires_at,
        applied_at,
    }
}

fn candidate_for(
    targets: &[GatewayReplicaRecoveryTarget],
    node_id: NodeId,
) -> Result<&GatewayPublication, Box<dyn std::error::Error>> {
    Ok(&targets
        .iter()
        .find(|target| target.publication.node_id == node_id)
        .ok_or("Gateway recovery candidate target disappeared")?
        .publication)
}

fn prior_for(
    targets: &[GatewayReplicaRecoveryTarget],
    node_id: NodeId,
) -> Result<&GatewayPublication, Box<dyn std::error::Error>> {
    Ok(targets
        .iter()
        .find(|target| target.publication.node_id == node_id)
        .and_then(|target| target.prior_publication.as_ref())
        .ok_or("Gateway recovery prior target disappeared")?)
}

fn recovery(
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> Result<&GatewayReplicaRecovery, Box<dyn std::error::Error>> {
    Ok(rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .and_then(|replica| replica.recovery.as_ref())
        .ok_or("Gateway replica recovery disappeared")?)
}

async fn recovery_target(
    repository: &PostgresEdgeRepository,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
) -> Result<GatewayReplicaRecoveryTarget, Box<dyn std::error::Error>> {
    repository
        .pending_gateway_replica_recoveries(100)
        .await?
        .into_iter()
        .find(|target| target.rollout.id == rollout_id && target.publication.node_id == node_id)
        .ok_or_else(|| "Gateway recovery target disappeared after restart".into())
}

async fn stage_observation(
    repository: &PostgresEdgeRepository,
    fixture: GatewayRolloutFixture,
    rollout: GatewayRollout,
    node_id: NodeId,
    command_id: NodeCommandId,
    issued_at: DateTime<Utc>,
) -> Result<GatewayRollout, Box<dyn std::error::Error>> {
    let expected_version = rollout.aggregate_version;
    let rollout = repository
        .stage_gateway_replica_recovery_observation(
            fixture.organization_id,
            rollout.id,
            node_id,
            expected_version,
            command_id,
            issued_at,
            issued_at + Duration::minutes(1),
        )
        .await?;
    assert_eq!(rollout.aggregate_version, expected_version + 1);
    Ok(rollout)
}

async fn record_observation(
    repository: &PostgresEdgeRepository,
    fixture: GatewayRolloutFixture,
    rollout: GatewayRollout,
    node_id: NodeId,
    observation: NodeGatewaySnapshotObservation,
) -> Result<GatewayRollout, Box<dyn std::error::Error>> {
    let expected_version = rollout.aggregate_version;
    let rollout = repository
        .record_gateway_replica_recovery_observation(
            fixture.organization_id,
            rollout.id,
            node_id,
            expected_version,
            observation,
        )
        .await?;
    assert_eq!(rollout.aggregate_version, expected_version + 1);
    Ok(rollout)
}
