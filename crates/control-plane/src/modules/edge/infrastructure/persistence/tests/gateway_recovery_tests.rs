use crate::modules::edge::domain::events::{GatewayRolloutStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayReplicaRecoveryState, GatewayReplicaRolloutState, GatewayRollout,
    GatewayRolloutPolicy, GatewayRolloutRollbackState, GatewayScope,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{
    AppliedGatewaySnapshot, GatewayAckState, GatewayManagementProtocol, GatewaySnapshot,
    GatewaySnapshotObservationState, NodeGatewayAck, NodeGatewaySnapshotObservation,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

#[tokio::test]
async fn gateway_replica_recovery_persists_every_physical_outcome_and_fails_stale_writers() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let nodes = (0..5).map(|_| NodeId::new()).collect::<Vec<_>>();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        nodes[0],
        nodes.clone(),
        GatewayRolloutPolicy::new(
            1,
            u32::try_from(nodes.len()).expect("bounded member count") - 1,
            nodes.len(),
        )
        .expect("recovery rollout policy"),
        now,
    )
    .expect("recovery Gateway scope");
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "gateway-recovery-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("scope members")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("persist recovery scope");

    let prior_publications = publications(&nodes, 1, None, now);
    let prior = stage_rollout(
        &repository,
        &scope,
        1,
        prior_publications.clone(),
        "gateway-recovery-prior",
    )
    .await;
    for publication in &prior.publications {
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(publication, now + Duration::seconds(1)),
                now + Duration::seconds(1) + Duration::microseconds(1),
            )
            .await
            .expect("project prior acknowledgement");
    }

    let candidate_issued_at = now + Duration::seconds(2);
    let candidate_publications = publications(&nodes, 2, Some(1), candidate_issued_at);
    let candidate = stage_rollout(
        &repository,
        &scope,
        2,
        candidate_publications.clone(),
        "gateway-recovery-candidate",
    )
    .await;
    let unavailable_at = candidate_publications[0].command_not_after + Duration::seconds(1);
    let failure = "Gateway command expired before exact acknowledgement";
    let mut rollout = candidate.rollout;
    for node_id in &nodes {
        let previous_version = rollout.aggregate_version;
        rollout = repository
            .mark_gateway_rollout_replica_unavailable(
                organization_id,
                rollout.id,
                *node_id,
                previous_version,
                failure,
                unavailable_at,
            )
            .await
            .expect("mark recovery member unavailable");
        assert_eq!(rollout.aggregate_version, previous_version + 1);
    }
    assert!(rollout.state.terminal());
    assert!(rollout.replicas.iter().all(|replica| {
        replica.state == GatewayReplicaRolloutState::Unavailable
            && replica.recovery.as_ref().is_some_and(|recovery| {
                recovery.state == GatewayReplicaRecoveryState::Required && recovery.attempt == 0
            })
    }));
    assert!(repository
        .pending_gateway_rollout_rollbacks(10)
        .await
        .expect("rollback scan before physical resolution")
        .is_empty());

    let pending = repository
        .pending_gateway_replica_recoveries(10)
        .await
        .expect("required recoveries");
    assert_eq!(pending.len(), nodes.len());
    for target in &pending {
        target.validate().expect("recovery target");
        assert_eq!(target.publication.revision, 2);
        assert_eq!(
            target
                .prior_publication
                .as_ref()
                .expect("known prior publication")
                .revision,
            1
        );
    }
    assert!(repository
        .pending_gateway_replica_recoveries(0)
        .await
        .is_err());

    let first_command = NodeCommandId::new();
    let first_issued_at = unavailable_at + Duration::seconds(1);
    let stale_version = rollout.aggregate_version;
    rollout = repository
        .stage_gateway_replica_recovery_observation(
            organization_id,
            rollout.id,
            nodes[0],
            stale_version,
            first_command,
            first_issued_at,
            first_issued_at + Duration::minutes(1),
        )
        .await
        .expect("stage first observation");
    assert_eq!(rollout.aggregate_version, stale_version + 1);
    let observing = repository
        .pending_gateway_replica_recoveries(10)
        .await
        .expect("restart-safe observing target")
        .into_iter()
        .find(|target| target.publication.node_id == nodes[0])
        .expect("observing recovery remains pending");
    assert_eq!(
        recovery(&observing.rollout, nodes[0]).state,
        GatewayReplicaRecoveryState::Observing
    );
    assert!(matches!(
        repository
            .record_gateway_replica_recovery_failure(
                organization_id,
                rollout.id,
                nodes[0],
                stale_version,
                first_command,
                "stale writer must lose",
                true,
                first_issued_at + Duration::seconds(1),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let first_candidate = candidate_for(&pending, nodes[0]);
    rollout = advance_observation(
        &repository,
        organization_id,
        rollout,
        nodes[0],
        observation(
            first_command,
            first_candidate,
            GatewaySnapshotObservationState::Applying,
            None,
            first_issued_at + Duration::seconds(1),
        ),
    )
    .await;
    assert_eq!(
        recovery(&rollout, nodes[0]).state,
        GatewayReplicaRecoveryState::Required
    );
    let second_command = NodeCommandId::new();
    let second_issued_at = first_issued_at + Duration::seconds(2);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[0],
        second_command,
        second_issued_at,
    )
    .await;
    let first_prior = prior_for(&pending, nodes[0]);
    rollout = advance_observation(
        &repository,
        organization_id,
        rollout,
        nodes[0],
        observation(
            second_command,
            first_candidate,
            GatewaySnapshotObservationState::NotApplied,
            Some(applied_snapshot(first_prior, now + Duration::seconds(1))),
            second_issued_at + Duration::seconds(1),
        ),
    )
    .await;
    assert_eq!(
        recovery(&rollout, nodes[0]).state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery(&rollout, nodes[0]).attempt, 2);
    assert_eq!(
        repository
            .gateway_scope(nodes[0])
            .await
            .expect("prior physical projection")
            .installed_revision,
        Some(1)
    );

    let candidate_command = NodeCommandId::new();
    let candidate_observation_at = first_issued_at + Duration::seconds(5);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[1],
        candidate_command,
        candidate_observation_at,
    )
    .await;
    let exact_candidate = candidate_for(&pending, nodes[1]);
    rollout = advance_observation(
        &repository,
        organization_id,
        rollout,
        nodes[1],
        observation(
            candidate_command,
            exact_candidate,
            GatewaySnapshotObservationState::Applied,
            Some(applied_snapshot(exact_candidate, candidate_observation_at)),
            candidate_observation_at + Duration::seconds(1),
        ),
    )
    .await;
    assert_eq!(
        recovery(&rollout, nodes[1]).state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(
        repository
            .gateway_scope(nodes[1])
            .await
            .expect("candidate physical projection")
            .installed_revision,
        Some(2)
    );

    let retry_command = NodeCommandId::new();
    let retry_issued_at = first_issued_at + Duration::seconds(8);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[2],
        retry_command,
        retry_issued_at,
    )
    .await;
    let previous_version = rollout.aggregate_version;
    rollout = repository
        .record_gateway_replica_recovery_failure(
            organization_id,
            rollout.id,
            nodes[2],
            previous_version,
            retry_command,
            "Gateway observation command lease expired",
            true,
            retry_issued_at + Duration::seconds(1),
        )
        .await
        .expect("record retryable observation failure");
    assert_eq!(rollout.aggregate_version, previous_version + 1);
    assert_eq!(
        recovery(&rollout, nodes[2]).state,
        GatewayReplicaRecoveryState::Required
    );
    let uninitialized_command = NodeCommandId::new();
    let uninitialized_at = retry_issued_at + Duration::seconds(2);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[2],
        uninitialized_command,
        uninitialized_at,
    )
    .await;
    rollout = advance_observation(
        &repository,
        organization_id,
        rollout,
        nodes[2],
        observation(
            uninitialized_command,
            candidate_for(&pending, nodes[2]),
            GatewaySnapshotObservationState::Uninitialized,
            None,
            uninitialized_at + Duration::seconds(1),
        ),
    )
    .await;
    assert_eq!(
        recovery(&rollout, nodes[2]).state,
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery(&rollout, nodes[2]).attempt, 2);
    assert_eq!(
        repository
            .gateway_scope(nodes[2])
            .await
            .expect("uninitialized physical projection")
            .installed_revision,
        None
    );
    assert!(repository
        .pending_gateway_rollout_rollbacks(10)
        .await
        .expect("rollback scan during partial physical resolution")
        .is_empty());

    let divergent_command = NodeCommandId::new();
    let divergent_at = first_issued_at + Duration::seconds(13);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[3],
        divergent_command,
        divergent_at,
    )
    .await;
    let divergent_candidate = candidate_for(&pending, nodes[3]);
    let mut unknown = applied_snapshot(divergent_candidate, divergent_at);
    unknown.revision += 1;
    unknown.expected_revision = Some(divergent_candidate.revision);
    unknown.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    rollout = advance_observation(
        &repository,
        organization_id,
        rollout,
        nodes[3],
        observation(
            divergent_command,
            divergent_candidate,
            GatewaySnapshotObservationState::NotApplied,
            Some(unknown),
            divergent_at + Duration::seconds(1),
        ),
    )
    .await;
    assert_eq!(
        recovery(&rollout, nodes[3]).state,
        GatewayReplicaRecoveryState::Diverged
    );
    assert_eq!(
        repository
            .find_gateway_rollout_rollback(organization_id, rollout.id)
            .await
            .expect("diverged rollback intent")
            .state,
        GatewayRolloutRollbackState::Diverged
    );
    assert!(repository
        .pending_gateway_rollout_rollbacks(10)
        .await
        .expect("diverged rollback scan")
        .is_empty());

    let terminal_command = NodeCommandId::new();
    let terminal_at = first_issued_at + Duration::seconds(16);
    rollout = stage_observation(
        &repository,
        organization_id,
        rollout,
        nodes[4],
        terminal_command,
        terminal_at,
    )
    .await;
    let previous_version = rollout.aggregate_version;
    rollout = repository
        .record_gateway_replica_recovery_failure(
            organization_id,
            rollout.id,
            nodes[4],
            previous_version,
            terminal_command,
            "Gateway observation protocol is unsupported",
            false,
            terminal_at + Duration::seconds(1),
        )
        .await
        .expect("record terminal observation failure");
    assert_eq!(rollout.aggregate_version, previous_version + 1);
    assert_eq!(
        recovery(&rollout, nodes[4]).state,
        GatewayReplicaRecoveryState::Diverged
    );

    assert!(repository
        .pending_gateway_replica_recoveries(10)
        .await
        .expect("terminal recovery scan")
        .is_empty());
    assert_eq!(
        repository
            .find_gateway_rollout(organization_id, rollout.id)
            .await
            .expect("restored recovery rollout"),
        rollout
    );
    assert!(rollout
        .replicas
        .iter()
        .all(|replica| replica.state == GatewayReplicaRolloutState::Unavailable));
}

async fn stage_rollout(
    repository: &InMemoryEdgeRepository,
    scope: &GatewayScope,
    generation: u64,
    publications: Vec<GatewayPublication>,
    key: &str,
) -> crate::modules::edge::domain::repositories::GatewayRolloutResult {
    let rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        scope,
        generation,
        &publications,
        publications[0].command_issued_at,
    )
    .expect("Gateway rollout");
    let mut expected_scope_versions = BTreeMap::new();
    for node_id in &scope.member_node_ids {
        expected_scope_versions.insert(
            *node_id,
            repository
                .gateway_scope(*node_id)
                .await
                .expect("physical scope")
                .aggregate_version,
        );
    }
    repository
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
            )
            .expect("rollout idempotency"),
            event: GatewayRolloutStaged::envelope(scope, &rollout).expect("rollout event"),
            route_event: None,
        })
        .await
        .expect("stage Gateway rollout")
}

fn publications(
    nodes: &[NodeId],
    revision: u64,
    expected_revision: Option<u64>,
    issued_at: DateTime<Utc>,
) -> Vec<GatewayPublication> {
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
            )
            .expect("Gateway snapshot");
            GatewayPublication::stage(
                *node_id,
                NodeCommandId::new(),
                correlation_id,
                snapshot,
                issued_at,
                issued_at + Duration::minutes(3),
            )
            .expect("Gateway publication")
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
    targets: &[crate::modules::edge::domain::repositories::GatewayReplicaRecoveryTarget],
    node_id: NodeId,
) -> &GatewayPublication {
    &targets
        .iter()
        .find(|target| target.publication.node_id == node_id)
        .expect("candidate recovery target")
        .publication
}

fn prior_for(
    targets: &[crate::modules::edge::domain::repositories::GatewayReplicaRecoveryTarget],
    node_id: NodeId,
) -> &GatewayPublication {
    targets
        .iter()
        .find(|target| target.publication.node_id == node_id)
        .and_then(|target| target.prior_publication.as_ref())
        .expect("prior recovery target")
}

fn recovery(
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> &crate::modules::edge::domain::GatewayReplicaRecovery {
    rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .and_then(|replica| replica.recovery.as_ref())
        .expect("Gateway replica recovery")
}

async fn stage_observation(
    repository: &InMemoryEdgeRepository,
    organization_id: OrganizationId,
    rollout: GatewayRollout,
    node_id: NodeId,
    command_id: NodeCommandId,
    issued_at: DateTime<Utc>,
) -> GatewayRollout {
    let previous_version = rollout.aggregate_version;
    let rollout = repository
        .stage_gateway_replica_recovery_observation(
            organization_id,
            rollout.id,
            node_id,
            previous_version,
            command_id,
            issued_at,
            issued_at + Duration::minutes(1),
        )
        .await
        .expect("stage recovery observation");
    assert_eq!(rollout.aggregate_version, previous_version + 1);
    rollout
}

async fn advance_observation(
    repository: &InMemoryEdgeRepository,
    organization_id: OrganizationId,
    rollout: GatewayRollout,
    node_id: NodeId,
    observation: NodeGatewaySnapshotObservation,
) -> GatewayRollout {
    let previous_version = rollout.aggregate_version;
    let rollout = repository
        .record_gateway_replica_recovery_observation(
            organization_id,
            rollout.id,
            node_id,
            previous_version,
            observation,
        )
        .await
        .expect("record recovery observation");
    assert_eq!(rollout.aggregate_version, previous_version + 1);
    rollout
}
