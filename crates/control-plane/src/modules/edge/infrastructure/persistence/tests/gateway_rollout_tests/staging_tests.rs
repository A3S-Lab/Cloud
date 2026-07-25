use super::*;

#[tokio::test]
async fn route_rollout_stages_every_projection_and_replays_without_partial_conflicts() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let first_scope = replicated_scope(
        organization_id,
        project_id,
        environment_id,
        [NodeId::new(), NodeId::new()],
        now,
    );
    persist_scope(&repository, &first_scope, "route-rollout-first").await;
    let route_id = RouteId::new();
    let first_bundle = route_rollout_bundle(
        &first_scope,
        route_id,
        1,
        "api.example.com",
        "route-rollout-first",
        now,
    );

    let staged = repository
        .stage_gateway_rollout(first_bundle.clone())
        .await
        .expect("stage Route rollout");
    let replayed = repository
        .stage_gateway_rollout(first_bundle)
        .await
        .expect("replay Route rollout");

    assert!(!staged.replayed);
    assert!(replayed.replayed);
    assert_eq!(replayed.route_replicas, staged.route_replicas);
    assert_eq!(replayed.publications, staged.publications);
    assert_eq!(replayed.certificates, staged.certificates);
    assert_eq!(
        staged.route_replicas.len(),
        first_scope.member_node_ids.len()
    );
    assert_eq!(staged.publications.len(), first_scope.member_node_ids.len());
    assert_eq!(staged.certificates.len(), first_scope.member_node_ids.len());
    assert!(staged
        .route_replicas
        .iter()
        .all(|route| route.id == route_id));
    let primary = staged
        .route_replicas
        .iter()
        .find(|route| route.gateway_node_id == first_scope.node_id)
        .expect("primary Route projection");
    assert_eq!(
        repository
            .find_route(organization_id, route_id)
            .await
            .expect("logical Route"),
        *primary
    );
    for node_id in &first_scope.member_node_ids {
        assert!(repository
            .active_routes(*node_id)
            .await
            .expect("active member routes")
            .is_empty());
        let physical_scope = repository
            .gateway_scope(*node_id)
            .await
            .expect("physical Gateway scope");
        assert_eq!(physical_scope.last_issued_revision, 1);
        assert_eq!(physical_scope.installed_revision, None);
        assert_eq!(physical_scope.aggregate_version, 1);
    }
    assert_eq!(
        repository
            .list_gateway_certificates(organization_id)
            .await
            .expect("staged certificates")
            .len(),
        first_scope.member_node_ids.len()
    );
    let events = repository.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "edge.gateway-rollout.staged")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "edge.route.publication-staged")
            .count(),
        1
    );

    let applied_node = first_scope
        .member_node_ids
        .iter()
        .copied()
        .find(|node_id| *node_id != first_scope.node_id)
        .expect("secondary member");
    let applied_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id == applied_node)
        .expect("secondary publication");
    let applied_certificate = staged
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied_node)
        .expect("secondary certificate");
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
        .expect("threshold acknowledgement");
    let ready = repository
        .find_gateway_rollout(organization_id, staged.rollout.id)
        .await
        .expect("ready Route rollout");
    assert_eq!(ready.state, GatewayRolloutState::Ready);
    assert!(ready.serves_traffic().expect("readiness"));
    let applied_projection = staged
        .route_replicas
        .iter()
        .find(|route| route.gateway_node_id == applied_node)
        .expect("secondary Route projection");
    let active_on_applied_member = repository
        .active_routes(applied_node)
        .await
        .expect("active secondary Route");
    assert_eq!(active_on_applied_member.len(), 1);
    assert_eq!(active_on_applied_member[0].id, applied_projection.id);
    assert_eq!(active_on_applied_member[0].state, RouteState::Active);
    assert!(repository
        .active_routes(first_scope.node_id)
        .await
        .expect("primary routes before acknowledgement")
        .is_empty());

    let rejected_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id == first_scope.node_id)
        .expect("primary publication");
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
        .expect("rejected primary acknowledgement");
    let degraded = repository
        .find_gateway_rollout(organization_id, staged.rollout.id)
        .await
        .expect("degraded Route rollout");
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert!(degraded.serves_traffic().expect("degraded readiness"));
    assert!(matches!(
        repository
            .find_gateway_rollout_rollback(organization_id, degraded.id)
            .await,
        Err(RepositoryError::NotFound)
    ));
    let logical_after_rollout = repository
        .find_route(organization_id, route_id)
        .await
        .expect("active logical Route");
    assert_eq!(logical_after_rollout.state, RouteState::Active);
    assert!(logical_after_rollout.activated_at.is_some());
    assert!(repository
        .active_routes(first_scope.node_id)
        .await
        .expect("rejected primary projection")
        .is_empty());

    let second_scope = replicated_scope(
        organization_id,
        project_id,
        environment_id,
        [NodeId::new(), NodeId::new()],
        now + Duration::seconds(1),
    );
    persist_scope(&repository, &second_scope, "route-rollout-second").await;
    let conflicting = route_rollout_bundle(
        &second_scope,
        route_id,
        1,
        "other.example.com",
        "route-rollout-second",
        now + Duration::seconds(1),
    );
    let conflicting_rollout_id = conflicting.rollout.id;
    let outbox_count = repository.outbox_events().await.len();
    let certificate_count = repository
        .list_gateway_certificates(organization_id)
        .await
        .expect("certificates before conflict")
        .len();

    assert!(matches!(
        repository.stage_gateway_rollout(conflicting).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(repository.outbox_events().await.len(), outbox_count);
    assert_eq!(
        repository
            .list_gateway_certificates(organization_id)
            .await
            .expect("certificates after conflict")
            .len(),
        certificate_count
    );
    assert_eq!(
        repository
            .list_routes(organization_id, project_id, environment_id)
            .await
            .expect("logical routes after conflict"),
        vec![logical_after_rollout]
    );
    assert!(matches!(
        repository
            .find_gateway_rollout(organization_id, conflicting_rollout_id)
            .await,
        Err(RepositoryError::NotFound)
    ));
    for node_id in &second_scope.member_node_ids {
        assert_eq!(
            repository
                .gateway_scope(*node_id)
                .await
                .expect("unchanged physical scope"),
            GatewayScopeState::empty(*node_id)
        );
    }
}

#[tokio::test]
async fn complete_replicated_snapshot_rebinds_every_retained_active_route() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let scope = replicated_scope(
        organization_id,
        project_id,
        environment_id,
        [NodeId::new(), NodeId::new()],
        now,
    );
    persist_scope(&repository, &scope, "route-rollout-rebinding").await;

    let first = repository
        .stage_gateway_rollout(route_rollout_bundle(
            &scope,
            RouteId::new(),
            1,
            "first.example.com",
            "route-rollout-rebinding-first",
            now,
        ))
        .await
        .expect("stage first Route rollout");
    for publication in &first.publications {
        let certificate = first
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .expect("first member certificate");
        super::super::issue(&repository, certificate, now + Duration::seconds(1)).await;
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(
                    publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(2),
                ),
                now + Duration::seconds(2),
            )
            .await
            .expect("apply first complete snapshot");
    }
    assert_eq!(
        repository
            .find_gateway_rollout(organization_id, first.rollout.id)
            .await
            .expect("first completed rollout")
            .state,
        GatewayRolloutState::Succeeded
    );

    let mut member_contexts = Vec::with_capacity(scope.member_node_ids.len());
    for node_id in &scope.member_node_ids {
        member_contexts.push(GatewayMemberSnapshotContext {
            scope: repository
                .gateway_scope(*node_id)
                .await
                .expect("installed physical scope"),
            active_routes: repository
                .active_routes(*node_id)
                .await
                .expect("first active Route"),
        });
    }
    let second = repository
        .stage_gateway_rollout(route_rollout_bundle_with_contexts(
            &scope,
            RouteId::new(),
            2,
            "second.example.com",
            "route-rollout-rebinding-second",
            member_contexts,
            now + Duration::seconds(3),
        ))
        .await
        .expect("stage second Route rollout");
    for publication in &second.publications {
        let certificate = second
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .expect("second member certificate");
        super::super::issue(&repository, certificate, now + Duration::seconds(4)).await;
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(
                    publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(5),
                ),
                now + Duration::seconds(5),
            )
            .await
            .expect("apply second complete snapshot");
    }
    assert_eq!(
        repository
            .find_gateway_rollout(organization_id, second.rollout.id)
            .await
            .expect("second completed rollout")
            .state,
        GatewayRolloutState::Succeeded
    );

    for publication in &second.publications {
        let certificate_id = second
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .expect("replacement member certificate")
            .id;
        let routes = repository
            .active_routes(publication.node_id)
            .await
            .expect("complete active Route projection");
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| {
            route.gateway_revision == Some(publication.revision)
                && route.gateway_command_id == Some(publication.command_id)
                && route.snapshot_digest.as_deref() == Some(&publication.snapshot_digest)
                && route.gateway_certificate_id == Some(certificate_id)
        }));
    }
}

#[tokio::test]
async fn exact_rollback_reuses_ready_certificates_and_rebinds_every_retained_route() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let members = [NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 0, members.len()).expect("exact policy"),
        now,
    )
    .expect("Gateway scope");
    persist_scope(&repository, &scope, "rollback-certificate-reuse").await;

    let retained = repository
        .stage_gateway_rollout(route_rollout_bundle(
            &scope,
            RouteId::new(),
            1,
            "retained.example.com",
            "rollback-certificate-reuse-retained",
            now,
        ))
        .await
        .expect("stage retained Route");
    for publication in &retained.publications {
        let certificate = retained
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .expect("retained certificate");
        super::super::issue(&repository, certificate, now + Duration::seconds(1)).await;
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(
                    publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(2),
                ),
                now + Duration::seconds(2),
            )
            .await
            .expect("activate retained Route");
    }

    let mut failed_contexts = Vec::new();
    for node_id in &scope.member_node_ids {
        failed_contexts.push(GatewayMemberSnapshotContext {
            scope: repository
                .gateway_scope(*node_id)
                .await
                .expect("installed scope"),
            active_routes: repository
                .active_routes(*node_id)
                .await
                .expect("retained Route"),
        });
    }
    let failed = repository
        .stage_gateway_rollout(route_rollout_bundle_with_contexts(
            &scope,
            RouteId::new(),
            2,
            "failed.example.com",
            "rollback-certificate-reuse-failed",
            failed_contexts,
            now + Duration::seconds(3),
        ))
        .await
        .expect("stage failed Route");
    let applied = &failed.publications[0];
    let applied_certificate = failed
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied.node_id)
        .expect("failed candidate certificate");
    super::super::issue(&repository, applied_certificate, now + Duration::seconds(4)).await;
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                applied,
                GatewayAckState::Applied,
                now + Duration::seconds(5),
            ),
            now + Duration::seconds(5),
        )
        .await
        .expect("partially apply failed Route");
    let rejected = failed
        .publications
        .iter()
        .find(|publication| publication.node_id != applied.node_id)
        .expect("rejected member");
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                rejected,
                GatewayAckState::Rejected,
                now + Duration::seconds(6),
            ),
            now + Duration::seconds(6),
        )
        .await
        .expect("reject failed Route");
    let failed_rollout = repository
        .find_gateway_rollout(organization_id, failed.rollout.id)
        .await
        .expect("failed rollout");
    let rollback = repository
        .find_gateway_rollout_rollback(organization_id, failed.rollout.id)
        .await
        .expect("required rollback");

    let mut rollback_contexts = Vec::new();
    for node_id in &scope.member_node_ids {
        let routes = repository
            .active_routes(*node_id)
            .await
            .expect("retained active routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname.as_str(), "retained.example.com");
        let certificate = repository
            .find_gateway_certificate(
                *node_id,
                routes[0]
                    .gateway_certificate_id
                    .expect("retained certificate binding"),
            )
            .await
            .expect("reusable ready certificate");
        assert_eq!(certificate.state, GatewayCertificateState::Ready);
        rollback_contexts.push(GatewayRollbackMemberSnapshotContext {
            scope: repository
                .gateway_scope(*node_id)
                .await
                .expect("observed physical scope"),
            active_routes: routes,
            reusable_certificate: Some(certificate),
        });
    }
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
        failed_rollout,
        rollback,
        member_contexts: rollback_contexts,
        issued_at: now + Duration::seconds(7),
    })
    .expect("compile retained-route rollback");
    assert!(compiled.certificates.is_empty());
    assert_eq!(compiled.reused_certificates.len(), members.len());
    assert!(compiled
        .publications
        .iter()
        .all(|publication| !publication.acl.contains("failed.example.com")));
    let rollback = repository
        .stage_gateway_rollout_rollback(
            compiled
                .stage_bundle()
                .expect("retained-route rollback bundle"),
        )
        .await
        .expect("stage retained-route rollback");
    for (index, publication) in rollback.publications.iter().enumerate() {
        let acknowledged_at = now + Duration::seconds(8 + i64::try_from(index).expect("index"));
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(publication, GatewayAckState::Applied, acknowledged_at),
                acknowledged_at,
            )
            .await
            .expect("acknowledge reused-certificate rollback");
    }
    for publication in &rollback.publications {
        let routes = repository
            .active_routes(publication.node_id)
            .await
            .expect("rebound retained Route");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname.as_str(), "retained.example.com");
        assert_eq!(routes[0].gateway_revision, Some(publication.revision));
        assert_eq!(routes[0].gateway_command_id, Some(publication.command_id));
        assert_eq!(
            routes[0].snapshot_digest.as_deref(),
            Some(publication.snapshot_digest.as_str())
        );
    }
}
