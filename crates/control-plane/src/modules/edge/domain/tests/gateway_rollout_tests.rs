use super::*;

#[test]
fn logical_gateway_scope_owns_one_environment_node_binding() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let node_id = NodeId::new();
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        node_id,
        Utc::now(),
    )
    .expect("Gateway scope");

    assert!(scope.owns(organization_id, project_id, environment_id, node_id));
    assert!(!scope.owns(organization_id, project_id, EnvironmentId::new(), node_id,));
    assert!(!scope.owns(organization_id, project_id, environment_id, NodeId::new(),));
}

#[test]
fn logical_gateway_scope_owns_bounded_replica_membership_and_rollout_policy() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let policy = GatewayRolloutPolicy::new(1, 1, 2).expect("replicated policy");
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        primary,
        vec![secondary, primary],
        policy,
        Utc::now(),
    )
    .expect("replicated Gateway scope");

    assert_eq!(scope.node_id, primary);
    assert_eq!(scope.member_node_ids, vec![primary, secondary]);
    assert_eq!(scope.membership_generation, 1);
    assert_eq!(
        scope
            .rollout_policy
            .required_ready(2)
            .expect("valid scope rollout policy"),
        1
    );
    assert!(scope.owns(organization_id, project_id, environment_id, primary));
    assert!(!scope.owns(organization_id, project_id, environment_id, secondary));
    assert!(scope.contains_member(primary));
    assert!(scope.contains_member(secondary));
    assert!(GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        primary,
        vec![primary, primary],
        policy,
        Utc::now(),
    )
    .is_err());
    assert!(GatewayRolloutPolicy::new(0, 0, 2).is_err());
    assert!(GatewayRolloutPolicy::new(1, 2, 2).is_err());
}

#[test]
fn replicated_gateway_rollout_requires_exact_per_member_terminal_evidence() {
    let now = Utc::now();
    let correlation_id = Uuid::now_v7();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let tertiary = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        primary,
        vec![primary, secondary, tertiary],
        GatewayRolloutPolicy::new(2, 1, 3).expect("rollout policy"),
        now,
    )
    .expect("replicated scope");
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| rollout_publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let mut rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("stage rollout");

    assert_eq!(rollout.required_ready().expect("valid rollout policy"), 2);
    assert!(!rollout
        .serves_traffic()
        .expect("valid pending rollout policy"));
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("first replica");
    assert_eq!(rollout.state, GatewayRolloutState::Pending);
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Applied,
            now + Duration::seconds(2),
        ))
        .expect("second replica");
    assert_eq!(rollout.state, GatewayRolloutState::Ready);
    assert!(rollout
        .serves_traffic()
        .expect("valid ready rollout policy"));
    rollout
        .mark_unavailable(
            publications[2].node_id,
            "Gateway did not become ready before the rollout deadline",
            now + Duration::seconds(3),
        )
        .expect("terminal unavailable result");
    assert_eq!(rollout.state, GatewayRolloutState::Degraded);
    assert_eq!(rollout.ready_replicas, 2);
    assert_eq!(rollout.unavailable_replicas, 1);
    assert!(rollout.completed_at.is_some());
    assert!(rollout
        .serves_traffic()
        .expect("valid degraded rollout policy"));

    let replay = rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("exact acknowledgement replay");
    assert!(!replay);
    let mut mismatched = rollout_acknowledgement(
        &publications[1],
        GatewayAckState::Applied,
        now + Duration::seconds(2),
    );
    mismatched.snapshot_digest = format!("sha256:{}", "0".repeat(64));
    let terminal = rollout.clone();
    assert!(rollout.acknowledge(&mismatched).is_err());
    assert_eq!(rollout, terminal);

    let mut exhausted =
        GatewayRollout::stage(GatewayRolloutId::new(), &scope, 2, &publications, now)
            .expect("stage exhausted rollout");
    exhausted.aggregate_version = u64::MAX;
    let unchanged = exhausted.clone();
    assert!(exhausted
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .is_err());
    assert_eq!(exhausted, unchanged);
}

#[test]
fn failed_gateway_rollout_requires_one_deterministic_exact_rollback_intent() {
    let now = Utc::now();
    let (scope, publications, mut failed) = failed_gateway_rollout(now);

    let rollback = GatewayRolloutRollback::required(&failed).expect("required rollback");
    assert_eq!(rollback.failed_rollout_id, failed.id);
    assert_eq!(rollback.gateway_scope_id, scope.id);
    assert_eq!(rollback.membership_generation, scope.membership_generation);
    assert_eq!(rollback.failed_generation, failed.generation);
    assert_eq!(rollback.rollback_generation, failed.generation + 1);
    assert_eq!(
        rollback.rollback_rollout_id,
        GatewayRolloutRollback::deterministic_rollout_id(failed.id)
    );
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Required);
    assert_eq!(rollback.aggregate_version, 1);
    assert!(rollback.blocks_scope());
    assert_eq!(
        GatewayRolloutRollback::required(&failed).expect("deterministic replay"),
        rollback
    );

    let mut serving_scope = scope.clone();
    serving_scope.rollout_policy = GatewayRolloutPolicy::new(1, 1, 2).expect("threshold policy");
    let mut serving = GatewayRollout::stage(
        GatewayRolloutId::new(),
        &serving_scope,
        2,
        &publications,
        now,
    )
    .expect("serving rollout");
    serving
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("ready member");
    serving
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Rejected,
            now + Duration::seconds(2),
        ))
        .expect("unavailable member");
    assert!(serving.serves_traffic().expect("valid threshold rollout"));
    assert!(GatewayRolloutRollback::required(&serving).is_err());

    let terminal = failed.clone();
    assert!(!failed
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("terminal acknowledgement replay"));
    assert_eq!(failed, terminal);
}

#[test]
fn gateway_rollout_rollback_succeeds_only_after_every_exact_member_acknowledgement() {
    let now = Utc::now();
    let (scope, _, failed) = failed_gateway_rollout(now);
    let mut rollback = GatewayRolloutRollback::required(&failed).expect("required rollback");
    let rollback_started_at =
        failed.completed_at.expect("failed completion") + Duration::seconds(1);
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| {
            rollout_publication_at(
                *node_id,
                rollback.rollback_rollout_id.as_uuid(),
                rollback.rollback_generation,
                Some(1),
                rollback_started_at,
            )
        })
        .collect::<Vec<_>>();
    let mut compensation = GatewayRollout::stage_rollback(
        rollback.rollback_rollout_id,
        &scope,
        rollback.rollback_generation,
        &publications,
        rollback_started_at,
    )
    .expect("exact rollback rollout");

    assert!(rollback
        .stage(&compensation)
        .expect("stage rollback intent"));
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Staged);
    assert!(rollback.blocks_scope());
    assert!(!rollback
        .stage(&compensation)
        .expect("replay rollback staging"));

    compensation
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            rollback_started_at + Duration::seconds(1),
        ))
        .expect("first rollback acknowledgement");
    assert_eq!(compensation.state, GatewayRolloutState::Pending);
    let before_incomplete = rollback.clone();
    assert!(rollback.succeed(&compensation).is_err());
    assert_eq!(rollback, before_incomplete);

    compensation
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Applied,
            rollback_started_at + Duration::seconds(2),
        ))
        .expect("second rollback acknowledgement");
    assert_eq!(compensation.state, GatewayRolloutState::Succeeded);
    assert!(rollback
        .succeed(&compensation)
        .expect("complete rollback intent"));
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Succeeded);
    assert!(!rollback.blocks_scope());
    assert!(!rollback
        .succeed(&compensation)
        .expect("replay rollback completion"));
}

#[test]
fn rejected_gateway_rollout_rollback_remains_durably_blocking() {
    let now = Utc::now();
    let (scope, _, failed) = failed_gateway_rollout(now);
    let mut rollback = GatewayRolloutRollback::required(&failed).expect("required rollback");
    let rollback_started_at =
        failed.completed_at.expect("failed completion") + Duration::seconds(1);
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| {
            rollout_publication_at(
                *node_id,
                rollback.rollback_rollout_id.as_uuid(),
                rollback.rollback_generation,
                Some(1),
                rollback_started_at,
            )
        })
        .collect::<Vec<_>>();
    let mut compensation = GatewayRollout::stage_rollback(
        rollback.rollback_rollout_id,
        &scope,
        rollback.rollback_generation,
        &publications,
        rollback_started_at,
    )
    .expect("exact rollback rollout");
    rollback
        .stage(&compensation)
        .expect("stage rollback intent");
    compensation
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            rollback_started_at + Duration::seconds(1),
        ))
        .expect("first rollback acknowledgement");
    compensation
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Rejected,
            rollback_started_at + Duration::seconds(2),
        ))
        .expect("rejected rollback acknowledgement");

    assert_eq!(compensation.state, GatewayRolloutState::Degraded);
    assert!(rollback
        .diverge(
            &compensation,
            "Gateway rejected an exact rollback member snapshot",
        )
        .expect("record rollback divergence"));
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Diverged);
    assert_eq!(
        rollback.failure.as_deref(),
        Some("Gateway rejected an exact rollback member snapshot")
    );
    assert!(rollback.blocks_scope());
    assert!(!rollback
        .diverge(
            &compensation,
            "Gateway rejected an exact rollback member snapshot",
        )
        .expect("replay rollback divergence"));
    assert!(rollback.succeed(&compensation).is_err());
}

#[test]
fn gateway_publication_canonicalizes_snapshot_validity_at_database_precision() {
    let node_id = NodeId::new();
    let issued_at = Utc
        .timestamp_opt(1_700_000_000, 123_456_789)
        .single()
        .expect("issue time");
    let expires_at = issued_at + Duration::hours(1);
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        issued_at,
        expires_at,
        "# exact timestamp snapshot",
    )
    .expect("Gateway snapshot");

    let publication = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        issued_at,
        issued_at + Duration::minutes(3),
    )
    .expect("Gateway publication");

    assert_eq!(
        publication.command_issued_at,
        canonical_timestamp(issued_at)
    );
    assert_eq!(
        publication.snapshot_expires_at,
        canonical_timestamp(expires_at)
    );
    let recovered = publication.snapshot().expect("recovered snapshot");
    assert_eq!(recovered.issued_at, canonical_timestamp(issued_at));
    assert_eq!(recovered.expires_at, canonical_timestamp(expires_at));
}

#[test]
fn gateway_publication_deadline_expiry_is_a_terminal_unavailable_outcome() {
    let now = Utc::now();
    let mut publication = rollout_publication(NodeId::new(), Uuid::now_v7(), now);
    let observed_at = publication.command_not_after + Duration::seconds(1);
    let failure = "Gateway command expired before exact acknowledgement";

    assert!(publication
        .mark_unavailable(failure, observed_at)
        .expect("mark publication unavailable"));
    assert_eq!(publication.state, GatewayPublicationState::Unavailable);
    assert_eq!(publication.failure.as_deref(), Some(failure));
    assert_eq!(
        publication.acknowledged_at,
        Some(canonical_timestamp(observed_at))
    );
    assert!(!publication
        .mark_unavailable(failure, observed_at)
        .expect("replay unavailable outcome"));

    let terminal = publication.clone();
    assert!(publication
        .acknowledge(&rollout_acknowledgement(
            &terminal,
            GatewayAckState::Applied,
            observed_at + Duration::seconds(1),
        ))
        .is_err());
    assert_eq!(publication, terminal);
}

#[test]
fn unavailable_gateway_replica_recovers_only_from_exact_observed_physical_state() {
    let now = Utc::now();
    let node_id = NodeId::new();
    let correlation_id = Uuid::now_v7();
    let prior = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        GatewaySnapshot::new(
            node_id.as_uuid(),
            1,
            None,
            now - Duration::minutes(10),
            now + Duration::minutes(50),
            "# known prior snapshot",
        )
        .expect("prior snapshot"),
        now - Duration::minutes(10),
        now - Duration::minutes(7),
    )
    .expect("prior publication");
    let candidate = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        GatewaySnapshot::new(
            node_id.as_uuid(),
            2,
            Some(1),
            now,
            now + Duration::hours(1),
            "# candidate snapshot",
        )
        .expect("candidate snapshot"),
        now,
        now + Duration::minutes(3),
    )
    .expect("candidate publication");
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        now,
    )
    .expect("Gateway scope");
    let mut rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        &scope,
        1,
        std::slice::from_ref(&candidate),
        now,
    )
    .expect("Gateway rollout");
    rollout
        .mark_unavailable(
            node_id,
            "Gateway command expired before exact acknowledgement",
            now + Duration::minutes(4),
        )
        .expect("mark unavailable");
    let recovery = rollout.replicas[0]
        .recovery
        .as_ref()
        .expect("required recovery");
    assert_eq!(recovery.state, GatewayReplicaRecoveryState::Required);
    assert_eq!(recovery.attempt, 0);

    let first_command = NodeCommandId::new();
    rollout
        .stage_recovery_observation(
            node_id,
            first_command,
            now + Duration::minutes(5),
            now + Duration::minutes(6),
        )
        .expect("stage first observation");
    let applying = gateway_snapshot_observation(
        first_command,
        &candidate,
        GatewaySnapshotObservationState::Applying,
        None,
        now + Duration::minutes(5) + Duration::seconds(1),
    );
    rollout
        .record_recovery_observation(node_id, &candidate, Some(&prior), applying)
        .expect("record inconclusive observation");
    let recovery = rollout.replicas[0]
        .recovery
        .as_ref()
        .expect("retry recovery");
    assert_eq!(recovery.state, GatewayReplicaRecoveryState::Required);
    assert_eq!(recovery.attempt, 1);

    let second_command = NodeCommandId::new();
    rollout
        .stage_recovery_observation(
            node_id,
            second_command,
            now + Duration::minutes(7),
            now + Duration::minutes(8),
        )
        .expect("stage second observation");
    let after_expiry = gateway_snapshot_observation(
        second_command,
        &candidate,
        GatewaySnapshotObservationState::Uninitialized,
        None,
        canonical_timestamp(now + Duration::minutes(8)) + Duration::microseconds(1),
    );
    let before_expired_observation = rollout.clone();
    assert!(rollout
        .record_recovery_observation(node_id, &candidate, Some(&prior), after_expiry)
        .is_err());
    assert_eq!(rollout, before_expired_observation);

    let observed_prior = gateway_snapshot_observation(
        second_command,
        &candidate,
        GatewaySnapshotObservationState::NotApplied,
        Some(applied_snapshot(&prior, now - Duration::minutes(9))),
        canonical_timestamp(now + Duration::minutes(7) + Duration::seconds(1))
            + Duration::nanoseconds(999),
    );
    let replayed_observation = observed_prior.clone();
    assert!(rollout
        .record_recovery_observation(node_id, &candidate, Some(&prior), observed_prior)
        .expect("record known prior"));
    let recovery = rollout.replicas[0]
        .recovery
        .as_ref()
        .expect("observed recovery");
    assert_eq!(recovery.state, GatewayReplicaRecoveryState::Observed);
    assert_eq!(recovery.attempt, 2);
    assert_eq!(
        rollout.replicas[0].state,
        GatewayReplicaRolloutState::Unavailable
    );
    assert_eq!(rollout.state, GatewayRolloutState::Degraded);
    let terminal_version = rollout.aggregate_version;
    assert!(!rollout
        .record_recovery_observation(node_id, &candidate, Some(&prior), replayed_observation,)
        .expect("replay canonicalized observation"));
    assert_eq!(rollout.aggregate_version, terminal_version);
}

#[test]
fn unknown_gateway_applied_revision_is_a_terminal_divergence() {
    let now = Utc::now();
    let node_id = NodeId::new();
    let candidate = rollout_publication(node_id, Uuid::now_v7(), now);
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        now,
    )
    .expect("Gateway scope");
    let mut rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        &scope,
        1,
        std::slice::from_ref(&candidate),
        now,
    )
    .expect("Gateway rollout");
    rollout
        .mark_unavailable(
            node_id,
            "Gateway command expired before exact acknowledgement",
            now + Duration::minutes(4),
        )
        .expect("mark unavailable");
    let command_id = NodeCommandId::new();
    rollout
        .stage_recovery_observation(
            node_id,
            command_id,
            now + Duration::minutes(5),
            now + Duration::minutes(6),
        )
        .expect("stage observation");
    let mut unknown = applied_snapshot(&candidate, now + Duration::seconds(1));
    unknown.revision = candidate.revision + 1;
    unknown.expected_revision = Some(candidate.revision);
    unknown.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    let observation = gateway_snapshot_observation(
        command_id,
        &candidate,
        GatewaySnapshotObservationState::NotApplied,
        Some(unknown),
        now + Duration::minutes(5) + Duration::seconds(1),
    );
    rollout
        .record_recovery_observation(node_id, &candidate, None, observation)
        .expect("record divergent observation");
    assert_eq!(
        rollout.replicas[0]
            .recovery
            .as_ref()
            .expect("diverged recovery")
            .state,
        GatewayReplicaRecoveryState::Diverged
    );
}

fn gateway_snapshot_observation(
    command_id: NodeCommandId,
    candidate: &GatewayPublication,
    state: GatewaySnapshotObservationState,
    applied: Option<AppliedGatewaySnapshot>,
    observed_at: chrono::DateTime<Utc>,
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
    applied_at: chrono::DateTime<Utc>,
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

#[test]
fn unavailable_rollout_projection_retains_ambiguous_physical_ownership() {
    let now = Utc::now();
    let mut projection = route(now);
    let command_id = NodeCommandId::new();
    projection
        .stage(1, command_id, format!("sha256:{}", "a".repeat(64)), now)
        .expect("stage Route projection");
    let observed_at = now + Duration::minutes(4);
    let failure = "Gateway command expired before exact acknowledgement";

    assert!(projection
        .mark_unavailable_from_gateway_rollout(failure, observed_at)
        .expect("mark Route projection unavailable"));
    assert_eq!(projection.state, RouteState::Unavailable);
    assert_eq!(projection.failure.as_deref(), Some(failure));
    assert_eq!(projection.activated_at, None);
    assert!(!projection
        .mark_unavailable_from_gateway_rollout(failure, observed_at)
        .expect("replay Route unavailability"));
}

#[test]
fn unavailable_gateway_delivery_fails_an_unready_certificate() {
    let now = Utc::now();
    let certificate_id = GatewayCertificateId::new();
    let request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec!["api.example.com".into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let mut certificate = GatewayCertificate::provision(
        certificate_id,
        OrganizationId::new(),
        NodeId::new(),
        vec![DomainClaimId::new()],
        1,
        NodeCommandId::new(),
        format!("sha256:{}", "a".repeat(64)),
        request,
        now,
    )
    .expect("provision certificate");
    let observed_at = now + Duration::minutes(4);
    let failure = "Gateway command expired before exact acknowledgement";

    assert!(certificate
        .mark_delivery_unavailable(failure, observed_at)
        .expect("fail unavailable certificate delivery"));
    assert_eq!(certificate.state, GatewayCertificateState::Failed);
    assert_eq!(certificate.failure.as_deref(), Some(failure));
    assert!(!certificate
        .mark_delivery_unavailable(failure, observed_at)
        .expect("replay unavailable certificate delivery"));
}

fn rollout_publication(
    node_id: NodeId,
    correlation_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> GatewayPublication {
    rollout_publication_at(node_id, correlation_id, 1, None, now)
}

fn rollout_publication_at(
    node_id: NodeId,
    correlation_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    now: chrono::DateTime<Utc>,
) -> GatewayPublication {
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        revision,
        expected_revision,
        now,
        now + Duration::hours(1),
        format!("# exact snapshot for {node_id}"),
    )
    .expect("Gateway snapshot");
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("Gateway publication")
}

fn failed_gateway_rollout(
    now: chrono::DateTime<Utc>,
) -> (GatewayScope, Vec<GatewayPublication>, GatewayRollout) {
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        primary,
        vec![primary, secondary],
        GatewayRolloutPolicy::new(2, 0, 2).expect("exact forward policy"),
        now,
    )
    .expect("replicated scope");
    let correlation_id = Uuid::now_v7();
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| rollout_publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let mut rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("forward rollout");
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("applied member");
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Rejected,
            now + Duration::seconds(2),
        ))
        .expect("rejected member");
    assert_eq!(rollout.state, GatewayRolloutState::Degraded);
    assert!(!rollout
        .serves_traffic()
        .expect("valid failed rollout policy"));
    (scope, publications, rollout)
}

fn rollout_acknowledgement(
    publication: &GatewayPublication,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
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
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "snapshot rejected".into()),
        acknowledged_at,
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}
