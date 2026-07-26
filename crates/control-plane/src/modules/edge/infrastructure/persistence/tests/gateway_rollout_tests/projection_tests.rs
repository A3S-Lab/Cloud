use super::*;

#[tokio::test]
async fn route_rollout_hides_partial_apply_and_rejects_logical_route_below_threshold() {
    let repository = InMemoryEdgeRepository::new();
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
    .expect("replicated Gateway scope");
    persist_scope(&repository, &scope, "route-rollout-below-threshold").await;
    let bundle = route_rollout_bundle(
        &scope,
        RouteId::new(),
        1,
        "threshold.example.com",
        "route-rollout-below-threshold",
        now,
    );
    let staged = repository
        .stage_gateway_rollout(bundle)
        .await
        .expect("stage Route rollout");
    let applied_publication = &staged.publications[0];
    let applied_certificate = staged
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied_publication.node_id)
        .expect("applied certificate");
    super::super::issue(&repository, applied_certificate, now + Duration::seconds(1)).await;
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                applied_publication,
                GatewayAckState::Applied,
                now + Duration::seconds(2),
            ),
            now + Duration::seconds(2),
        )
        .await
        .expect("partial apply");
    assert_eq!(
        repository
            .find_route(organization_id, staged.route_replicas[0].id)
            .await
            .expect("publishing logical Route")
            .state,
        RouteState::Publishing
    );
    assert!(repository
        .active_routes(applied_publication.node_id)
        .await
        .expect("hidden partial apply")
        .is_empty());

    let rejected_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id != applied_publication.node_id)
        .expect("rejected publication");
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                rejected_publication,
                GatewayAckState::Rejected,
                now + Duration::seconds(3),
            ),
            now + Duration::seconds(3),
        )
        .await
        .expect("terminal rejection");
    let degraded = repository
        .find_gateway_rollout(organization_id, staged.rollout.id)
        .await
        .expect("degraded rollout");
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert!(!degraded.serves_traffic().expect("failed threshold"));
    let rollback = repository
        .find_gateway_rollout_rollback(organization_id, degraded.id)
        .await
        .expect("durable exact rollback intent");
    assert_eq!(rollback.state, GatewayRolloutRollbackState::Required);
    assert_eq!(rollback.failed_rollout_id, degraded.id);
    assert_eq!(rollback.gateway_scope_id, scope.id);
    assert_eq!(
        rollback.required_at,
        degraded.completed_at.expect("completion")
    );
    assert!(rollback.blocks_scope());
    let logical = repository
        .find_route(organization_id, staged.route_replicas[0].id)
        .await
        .expect("rejected logical Route");
    assert_eq!(logical.state, RouteState::Rejected);
    assert_eq!(
        logical.failure.as_deref(),
        Some("Gateway rollout did not reach its readiness threshold")
    );
    for node_id in members {
        assert!(repository
            .active_routes(node_id)
            .await
            .expect("no authoritative Route below threshold")
            .is_empty());
        assert_eq!(
            repository
                .gateway_route_owner(node_id, "threshold.example.com", "/")
                .await,
            Some(staged.route_replicas[0].id),
            "failed physical ownership must survive until exact rollback completes"
        );
    }

    let rollback_contexts =
        futures_util::future::try_join_all(scope.member_node_ids.iter().map(|node_id| {
            let repository = &repository;
            let node_id = *node_id;
            async move {
                Ok::<_, RepositoryError>(GatewayRollbackMemberSnapshotContext {
                    scope: repository.gateway_scope(node_id).await?,
                    active_routes: repository.active_routes(node_id).await?,
                    reusable_certificate: None,
                })
            }
        }))
        .await
        .expect("rollback member contexts");
    let compiled = GatewayRolloutRollbackCompiler::new(
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
    .expect("rollback compiler")
    .compile(CompileGatewayRolloutRollback {
        scope: scope.clone(),
        failed_rollout: degraded,
        rollback,
        member_contexts: rollback_contexts,
        issued_at: now + Duration::seconds(4),
    })
    .expect("compile exact rollback");
    let staged_rollback = repository
        .stage_gateway_rollout_rollback(
            compiled
                .stage_bundle()
                .expect("exact rollback stage bundle"),
        )
        .await
        .expect("stage exact rollback");
    assert!(!staged_rollback.replayed);
    assert!(staged_rollback.certificates.is_empty());
    assert!(staged_rollback.reused_certificates.is_empty());
    for (index, publication) in staged_rollback.publications.iter().enumerate() {
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(
                    publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(5 + i64::try_from(index).expect("index")),
                ),
                now + Duration::seconds(5 + i64::try_from(index).expect("index")),
            )
            .await
            .expect("exact rollback acknowledgement");
        for node_id in members {
            let expected = if index + 1 == staged_rollback.publications.len() {
                None
            } else {
                Some(staged.route_replicas[0].id)
            };
            assert_eq!(
                repository
                    .gateway_route_owner(node_id, "threshold.example.com", "/")
                    .await,
                expected,
                "failed ownership releases only after every rollback acknowledgement"
            );
        }
    }
    let completed_rollback = repository
        .find_gateway_rollout_rollback(organization_id, staged.rollout.id)
        .await
        .expect("completed exact rollback");
    assert_eq!(
        completed_rollback.state,
        GatewayRolloutRollbackState::Succeeded
    );
    assert!(!completed_rollback.blocks_scope());
}

#[tokio::test]
async fn route_rollout_expiry_terminalizes_all_projections_without_releasing_ownership() {
    let repository = InMemoryEdgeRepository::new();
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
    .expect("replicated Gateway scope");
    persist_scope(&repository, &scope, "route-rollout-expiry").await;
    let staged = repository
        .stage_gateway_rollout(route_rollout_bundle(
            &scope,
            RouteId::new(),
            1,
            "expiry.example.com",
            "route-rollout-expiry",
            now,
        ))
        .await
        .expect("stage Route rollout");
    let applied_publication = &staged.publications[0];
    let applied_certificate = staged
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied_publication.node_id)
        .expect("applied certificate");
    super::super::issue(&repository, applied_certificate, now + Duration::seconds(1)).await;
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                applied_publication,
                GatewayAckState::Applied,
                now + Duration::seconds(2),
            ),
            now + Duration::seconds(2),
        )
        .await
        .expect("partial apply");

    let unavailable_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id != applied_publication.node_id)
        .expect("unavailable publication");
    let pending = repository
        .find_gateway_rollout(organization_id, staged.rollout.id)
        .await
        .expect("pending rollout");
    let observed_at = unavailable_publication.command_not_after + Duration::seconds(1);
    let failure = "Gateway command expired before exact acknowledgement";
    let degraded = repository
        .mark_gateway_rollout_replica_unavailable(
            organization_id,
            staged.rollout.id,
            unavailable_publication.node_id,
            pending.aggregate_version,
            failure,
            observed_at,
        )
        .await
        .expect("expire rollout member");

    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert!(!degraded.serves_traffic().expect("failed threshold"));
    assert_eq!(
        repository
            .find_gateway_rollout_rollback(organization_id, degraded.id)
            .await
            .expect("durable rollback intent")
            .state,
        GatewayRolloutRollbackState::Required
    );
    assert_eq!(
        repository
            .find_route(organization_id, staged.route_replicas[0].id)
            .await
            .expect("rejected logical Route")
            .state,
        RouteState::Rejected
    );
    assert_eq!(
        repository
            .gateway_publication(
                unavailable_publication.node_id,
                unavailable_publication.revision,
            )
            .await
            .expect("terminal publication")
            .state,
        GatewayPublicationState::Unavailable
    );
    assert_eq!(
        repository
            .gateway_route_projection(staged.rollout.id, unavailable_publication.node_id)
            .await
            .expect("unavailable Route projection")
            .state,
        RouteState::Unavailable
    );
    assert_eq!(
        repository
            .list_gateway_certificates(organization_id)
            .await
            .expect("rollout certificates")
            .into_iter()
            .find(|certificate| certificate.node_id == unavailable_publication.node_id)
            .expect("unavailable member certificate")
            .state,
        GatewayCertificateState::Failed
    );
    assert_eq!(
        repository
            .gateway_route_owner(unavailable_publication.node_id, "expiry.example.com", "/",)
            .await,
        Some(staged.route_replicas[0].id)
    );
}
