use super::*;

#[tokio::test]
async fn route_publication_rejects_cross_environment_and_wrong_node_gateway_scopes() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let workload_id = WorkloadId::new();
    let now = Utc::now();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: fixed_target(workload_id, revision_id, node_id),
        }),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");
    let domain_claim_id = verified_claim(
        &routes,
        organization_id,
        project_id,
        environment_id,
        "scope.example.com",
        now,
    )
    .await;
    let cross_environment_scope = gateway_scope(
        &routes,
        organization_id,
        project_id,
        EnvironmentId::new(),
        node_id,
        now,
    )
    .await;
    let cross_environment = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                cross_environment_scope,
                revision_id,
                domain_claim_id,
                "scope.example.com",
                "cross-environment",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("cross-environment scope");
    assert!(matches!(cross_environment, ApplicationError::Conflict(_)));

    let wrong_node_scope = gateway_scope(
        &routes,
        organization_id,
        project_id,
        environment_id,
        NodeId::new(),
        now,
    )
    .await;
    let wrong_node = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                wrong_node_scope,
                revision_id,
                domain_claim_id,
                "scope.example.com",
                "wrong-node",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("wrong-node scope");
    assert!(matches!(wrong_node, ApplicationError::Conflict(_)));
    assert!(routes
        .list_routes(organization_id, project_id, environment_id)
        .await
        .expect("routes")
        .is_empty());
    assert!(queue.commands.lock().await.is_empty());
}

#[tokio::test]
async fn route_publication_stages_and_replays_every_replicated_gateway_member() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let workload_id = WorkloadId::new();
    let members = [NodeId::new(), NodeId::new()];
    let now = Utc::now();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len()).expect("rollout policy"),
        now,
    )
    .expect("replicated Gateway scope");
    routes
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "test-replicated-gateway-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("member identities")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("create scope");
    let claim_id = verified_claim(
        &routes,
        organization_id,
        project_id,
        environment_id,
        "replicated.example.com",
        now,
    )
    .await;
    let target_reader = Arc::new(ReplicatedTargetReader {
        workload_id,
        revision_id,
        observed_at: now,
        calls: AtomicUsize::new(0),
    });
    let handler = PublishRouteHandler::new(
        routes.clone(),
        target_reader.clone(),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");

    let first = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                scope.id,
                revision_id,
                claim_id,
                "replicated.example.com",
                "replicated-route",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("replicated Route publication");
    assert!(!first.publication.replayed);
    assert!(!first.command_replayed);
    assert_eq!(first.publication.route.gateway_node_id, scope.node_id);
    assert_eq!(first.publication.route.gateway_scope_id, scope.id);
    assert_eq!(
        first.publication.route.hostname.as_str(),
        "replicated.example.com"
    );
    let dispatches = routes
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("pending rollout");
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].publications.len(), members.len());
    assert_eq!(
        dispatches[0]
            .publications
            .iter()
            .map(|publication| publication.node_id)
            .collect::<std::collections::BTreeSet<_>>(),
        members.into_iter().collect()
    );
    assert_eq!(queue.commands.lock().await.len(), members.len());
    assert_eq!(target_reader.calls.load(Ordering::SeqCst), 1);

    let replay = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                scope.id,
                revision_id,
                claim_id,
                "replicated.example.com",
                "replicated-route",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("replayed replicated Route publication");
    assert!(replay.publication.replayed);
    assert!(replay.command_replayed);
    assert_eq!(replay.publication.route, first.publication.route);
    assert_eq!(
        replay.publication.certificate,
        first.publication.certificate
    );
    assert_eq!(
        replay.publication.publication,
        first.publication.publication
    );
    assert_eq!(queue.commands.lock().await.len(), members.len());
    assert_eq!(
        target_reader.calls.load(Ordering::SeqCst),
        1,
        "idempotency replay must not resolve mutable targets again"
    );
    assert_eq!(
        routes
            .list_routes(organization_id, project_id, environment_id)
            .await
            .expect("logical Routes"),
        vec![first.publication.route]
    );
}
