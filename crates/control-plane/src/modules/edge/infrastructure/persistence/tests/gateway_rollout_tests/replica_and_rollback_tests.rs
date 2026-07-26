use super::*;

#[tokio::test]
async fn replicated_rollout_persists_independent_acknowledgements_and_explicit_degradation() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let tertiary = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        primary,
        vec![primary, secondary, tertiary],
        GatewayRolloutPolicy::new(2, 1, 3).expect("rollout policy"),
        now,
    )
    .expect("Gateway scope");
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "gateway-rollout-scopes",
                "replicated-scope",
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("member identities")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("create scope");
    let correlation_id = Uuid::now_v7();
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("stage aggregate");
    let bundle = StageGatewayRollout {
        scope: scope.clone(),
        rollout: rollout.clone(),
        route_replicas: Vec::new(),
        publications: publications.clone(),
        certificates: Vec::new(),
        expected_scope_versions: scope
            .member_node_ids
            .iter()
            .map(|node_id| (*node_id, 0))
            .collect::<BTreeMap<_, _>>(),
        idempotency: IdempotencyRequest::new(
            format!("gateway-scopes/{}/rollouts", scope.id),
            "replicated-rollout",
            rollout.id.to_string().as_bytes(),
        )
        .expect("rollout idempotency"),
        event: GatewayRolloutStaged::envelope(&scope, &rollout).expect("rollout event"),
        route_event: None,
    };
    let staged = repository
        .stage_gateway_rollout(bundle.clone())
        .await
        .expect("stage rollout");
    let replay = repository
        .stage_gateway_rollout(bundle)
        .await
        .expect("replay rollout");
    assert!(!staged.replayed);
    assert!(replay.replayed);
    assert_eq!(staged.rollout, rollout);
    let dispatches = repository
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("pending rollout dispatches");
    assert_eq!(dispatches.len(), 1);
    dispatches[0].validate().expect("dispatch target");
    assert_eq!(dispatches[0].rollout, rollout);
    assert_eq!(dispatches[0].publications.len(), 3);

    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[0],
                GatewayAckState::Applied,
                now + Duration::seconds(1),
            ),
            now + Duration::seconds(1),
        )
        .await
        .expect("first acknowledgement");
    let pending = repository
        .find_gateway_rollout(organization_id, rollout.id)
        .await
        .expect("pending rollout");
    assert_eq!(pending.state, GatewayRolloutState::Pending);
    assert_eq!(pending.ready_replicas, 1);
    assert_eq!(
        repository
            .pending_gateway_rollout_dispatches(10)
            .await
            .expect("remaining pending rollout dispatches")[0]
            .publications
            .len(),
        2
    );

    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[1],
                GatewayAckState::Applied,
                now + Duration::seconds(2),
            ),
            now + Duration::seconds(2),
        )
        .await
        .expect("second acknowledgement");
    let ready = repository
        .find_gateway_rollout(organization_id, rollout.id)
        .await
        .expect("ready rollout");
    assert_eq!(ready.state, GatewayRolloutState::Ready);
    assert!(ready.serves_traffic().expect("readiness"));
    assert_eq!(
        repository
            .pending_gateway_rollout_dispatches(10)
            .await
            .expect("ready rollout dispatches")[0]
            .publications,
        vec![publications[2].clone()]
    );

    let degraded = repository
        .mark_gateway_rollout_replica_unavailable(
            organization_id,
            rollout.id,
            publications[2].node_id,
            ready.aggregate_version,
            "Gateway missed the rollout readiness deadline",
            publications[2].command_not_after + Duration::seconds(1),
        )
        .await
        .expect("degraded rollout");
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert_eq!(degraded.ready_replicas, 2);
    assert_eq!(degraded.unavailable_replicas, 1);
    assert!(degraded.serves_traffic().expect("degraded readiness"));
    assert!(repository
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("terminal rollout dispatches")
        .is_empty());
    assert!(matches!(
        repository
            .mark_gateway_rollout_replica_unavailable(
                organization_id,
                rollout.id,
                publications[2].node_id,
                ready.aggregate_version,
                "Gateway missed the rollout readiness deadline",
                publications[2].command_not_after + Duration::seconds(1),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(repository
        .pending_gateway_rollout_dispatches(0)
        .await
        .is_err());
}

#[tokio::test]
async fn rollback_reconciler_stages_only_one_exact_compensation_after_physical_resolution() {
    let repository = Arc::new(InMemoryEdgeRepository::new());
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let members = [NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 1, members.len()).expect("rollout policy"),
        now,
    )
    .expect("Gateway scope");
    persist_scope(&repository, &scope, "rollback-reconciler-scope").await;
    let correlation_id = Uuid::now_v7();
    let publications = members
        .iter()
        .map(|node_id| publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("failed rollout");
    repository
        .stage_gateway_rollout(StageGatewayRollout {
            scope: scope.clone(),
            rollout: rollout.clone(),
            route_replicas: Vec::new(),
            publications: publications.clone(),
            certificates: Vec::new(),
            expected_scope_versions: members.iter().map(|node_id| (*node_id, 0)).collect(),
            idempotency: IdempotencyRequest::new(
                format!("gateway-scopes/{}/rollouts", scope.id),
                "rollback-reconciler-failed",
                rollout.id.to_string().as_bytes(),
            )
            .expect("rollout idempotency"),
            event: GatewayRolloutStaged::envelope(&scope, &rollout).expect("rollout event"),
            route_event: None,
        })
        .await
        .expect("stage failed rollout");
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[0],
                GatewayAckState::Applied,
                now + Duration::seconds(1),
            ),
            now + Duration::seconds(1),
        )
        .await
        .expect("apply one member");
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[1],
                GatewayAckState::Rejected,
                now + Duration::seconds(2),
            ),
            now + Duration::seconds(2),
        )
        .await
        .expect("reject one member");
    assert_eq!(
        repository
            .pending_gateway_rollout_rollbacks(10)
            .await
            .expect("ready rollback targets")
            .len(),
        1
    );

    let repository_port: Arc<dyn IEdgeRepository> = repository.clone();
    let reconciler = GatewayRolloutRollbackReconciler::new(
        repository_port,
        GatewayRolloutRollbackCompiler::new(
            GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
                entrypoint_address: "0.0.0.0:8081".into(),
                management_address: "127.0.0.1:9090".into(),
                management_path_prefix: "/api/gateway".into(),
                management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
                upstream_request_timeout_ms: 30_000,
                certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
                managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
            })
            .expect("snapshot compiler"),
            Duration::minutes(3),
            Duration::hours(24),
        )
        .expect("rollback compiler"),
        std::time::Duration::from_millis(10),
        10,
    )
    .expect("rollback reconciler");
    let report = reconciler
        .run_once(now + Duration::seconds(3))
        .await
        .expect("reconcile rollback");
    assert_eq!(report.required_rollbacks, 1);
    assert_eq!(report.staged_rollbacks, 1);
    assert_eq!(report.replayed_rollbacks, 0);
    assert!(report.failures.is_empty());

    let rollback = repository
        .find_gateway_rollout_rollback(organization_id, rollout.id)
        .await
        .expect("staged rollback intent");
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Staged);
    let dispatches = repository
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("rollback dispatches");
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].rollout.id, rollback.rollback_rollout_id);
    assert_eq!(dispatches[0].publications.len(), members.len());
    assert!(dispatches[0]
        .publications
        .iter()
        .all(|publication| publication.certificate_request.is_none()));
    assert_eq!(
        reconciler
            .run_once(now + Duration::seconds(4))
            .await
            .expect("replay scan")
            .required_rollbacks,
        0
    );
}
