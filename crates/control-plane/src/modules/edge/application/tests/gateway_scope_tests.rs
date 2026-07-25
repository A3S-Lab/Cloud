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
async fn route_publication_never_partially_applies_a_replicated_gateway_scope() {
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
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: fixed_target(workload_id, revision_id, members[0]),
        }),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");

    let error = handler
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
        .expect_err("partial replicated publication");

    assert!(matches!(error, ApplicationError::Conflict(_)));
    assert!(routes
        .list_routes(organization_id, project_id, environment_id)
        .await
        .expect("routes")
        .is_empty());
    assert!(queue.commands.lock().await.is_empty());
}
