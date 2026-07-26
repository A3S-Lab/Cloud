use super::*;

pub(super) async fn exercise_retained_route_rebinding(
    repository: &PostgresEdgeRepository,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = persisted_route_scope(
        repository,
        fixture,
        members,
        "postgres-route-rollout-rebinding-scope",
        now + Duration::seconds(5),
    )
    .await?;
    let first = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                RouteId::new(),
                "first-rebinding.example.com",
                "postgres-route-rollout-rebinding-first",
                now + Duration::seconds(5) + Duration::milliseconds(1),
            )
            .await?,
        )
        .await?;
    for publication in &first.publications {
        let certificate = first
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("first rebinding rollout omitted a member certificate")?;
        issue_certificate(repository, certificate, now + Duration::seconds(6)).await?;
        assert!(
            repository
                .project_gateway_acknowledgement(
                    &route_acknowledgement(
                        publication,
                        GatewayAckState::Applied,
                        now + Duration::seconds(7),
                    ),
                    now + Duration::seconds(7) + Duration::microseconds(1),
                )
                .await?
        );
    }
    assert_eq!(
        repository
            .find_gateway_rollout(fixture.organization_id, first.rollout.id)
            .await?
            .state,
        GatewayRolloutState::Succeeded
    );

    let second = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                RouteId::new(),
                "second-rebinding.example.com",
                "postgres-route-rollout-rebinding-second",
                now + Duration::seconds(8),
            )
            .await?,
        )
        .await?;
    for publication in &second.publications {
        let certificate = second
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("second rebinding rollout omitted a member certificate")?;
        issue_certificate(repository, certificate, now + Duration::seconds(9)).await?;
        assert!(
            repository
                .project_gateway_acknowledgement(
                    &route_acknowledgement(
                        publication,
                        GatewayAckState::Applied,
                        now + Duration::seconds(10),
                    ),
                    now + Duration::seconds(10) + Duration::microseconds(1),
                )
                .await?
        );
    }
    assert_eq!(
        repository
            .find_gateway_rollout(fixture.organization_id, second.rollout.id)
            .await?
            .state,
        GatewayRolloutState::Succeeded
    );
    for publication in &second.publications {
        let certificate_id = second
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("replacement rollout omitted a member certificate")?
            .id;
        let routes = repository.active_routes(publication.node_id).await?;
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| {
            route.gateway_revision == Some(publication.revision)
                && route.gateway_command_id == Some(publication.command_id)
                && route.snapshot_digest.as_deref() == Some(&publication.snapshot_digest)
                && route.gateway_certificate_id == Some(certificate_id)
        }));
    }
    Ok(())
}

pub(super) async fn exercise_exact_route_rollback(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 1, members.len())?,
        now,
    )?;
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "postgres-exact-rollback-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;

    let retained = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                RouteId::new(),
                "retained-rollback.example.com",
                "postgres-exact-rollback-retained",
                now + Duration::seconds(1),
            )
            .await?,
        )
        .await?;
    for publication in &retained.publications {
        let certificate = retained
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("retained rollback rollout omitted a certificate")?;
        issue_certificate(repository, certificate, now + Duration::seconds(2)).await?;
        let acknowledgement = route_acknowledgement(
            publication,
            GatewayAckState::Applied,
            now + Duration::seconds(3),
        );
        assert!(
            repository
                .project_gateway_acknowledgement(
                    &acknowledgement,
                    now + Duration::seconds(3) + Duration::microseconds(1),
                )
                .await?
        );
    }
    assert_eq!(
        repository
            .find_gateway_rollout(fixture.organization_id, retained.rollout.id)
            .await?
            .state,
        GatewayRolloutState::Succeeded
    );

    let failed_route_id = RouteId::new();
    let failed = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                failed_route_id,
                "failed-rollback.example.com",
                "postgres-exact-rollback-failed",
                now + Duration::seconds(4),
            )
            .await?,
        )
        .await?;
    let applied_publication = &failed.publications[0];
    let applied_certificate = failed
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied_publication.node_id)
        .ok_or("failed rollback rollout omitted its applied certificate")?;
    issue_certificate(repository, applied_certificate, now + Duration::seconds(5)).await?;
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    applied_publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(6),
                ),
                now + Duration::seconds(6) + Duration::microseconds(1),
            )
            .await?
    );
    let rejected_publication = &failed.publications[1];
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    rejected_publication,
                    GatewayAckState::Rejected,
                    now + Duration::seconds(7),
                ),
                now + Duration::seconds(7) + Duration::microseconds(1),
            )
            .await?
    );
    let failed_rollout = repository
        .find_gateway_rollout(fixture.organization_id, failed.rollout.id)
        .await?;
    assert_eq!(failed_rollout.state, GatewayRolloutState::Degraded);
    assert!(!failed_rollout.serves_traffic()?);
    assert_eq!(
        repository
            .find_route(fixture.organization_id, failed_route_id)
            .await?
            .state,
        RouteState::Rejected
    );
    let required = repository
        .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
        .await?;
    assert_eq!(required.state, GatewayRolloutRollbackState::Required);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_ownership where gateway_rollout_id = ",
                )
                .bind(failed.rollout.id.as_uuid()),
            )
            .await?,
        i64::try_from(members.len())?
    );
    assert_eq!(
        repository
            .pending_gateway_rollout_rollbacks(10)
            .await?
            .len(),
        1
    );

    let rollback_repository: Arc<dyn IEdgeRepository> =
        Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let rollback_reconciler = GatewayRolloutRollbackReconciler::new(
        Arc::clone(&rollback_repository),
        GatewayRolloutRollbackCompiler::new(
            GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
                entrypoint_address: "0.0.0.0:8081".into(),
                management_address: "127.0.0.1:9090".into(),
                management_path_prefix: "/api/gateway".into(),
                management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
                upstream_request_timeout_ms: 30_000,
                certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
                managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
            })?,
            Duration::minutes(3),
            Duration::hours(24),
        )?,
        std::time::Duration::from_secs(1),
        10,
    )?;
    let report = rollback_reconciler
        .run_once(now + Duration::seconds(8))
        .await?;
    assert_eq!(report.required_rollbacks, 1);
    assert_eq!(report.staged_rollbacks, 1);
    assert_eq!(report.replayed_rollbacks, 0);
    assert!(report.failures.is_empty());
    assert_eq!(
        rollback_reconciler
            .run_once(now + Duration::seconds(8) + Duration::microseconds(1))
            .await?
            .required_rollbacks,
        0
    );

    let restarted = PostgresEdgeRepository::new(executor.clone());
    let staged_rollback = restarted
        .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
        .await?;
    assert_eq!(staged_rollback.state, GatewayRolloutRollbackState::Staged);
    let rollback_dispatch = restarted
        .pending_gateway_rollout_dispatches(10)
        .await?
        .into_iter()
        .find(|target| target.rollout.id == staged_rollback.rollback_rollout_id)
        .ok_or("staged exact rollback omitted its dispatch target")?;
    assert_eq!(rollback_dispatch.publications.len(), members.len());
    let reused_certificate_ids = rollback_dispatch
        .publications
        .iter()
        .map(|publication| {
            publication
                .certificate_request
                .as_ref()
                .map(|request| request.certificate_id)
                .ok_or("TLS rollback publication omitted its reused certificate")
        })
        .collect::<Result<Vec<_>, _>>()?;
    for publication in &rollback_dispatch.publications {
        assert_eq!(
            database
                .fetch_one_as(
                    sql_query::<i64>(
                        "select count(*) from gateway_certificates where gateway_command_id = ",
                    )
                    .bind(publication.command_id.as_uuid()),
                )
                .await?,
            0,
            "exact rollback must not replace a still-valid certificate"
        );
    }

    for (index, publication) in rollback_dispatch.publications.iter().enumerate() {
        let acknowledged_at = now + Duration::seconds(9 + i64::try_from(index)?);
        let acknowledgement =
            route_acknowledgement(publication, GatewayAckState::Applied, acknowledged_at);
        assert!(
            restarted
                .project_gateway_acknowledgement(
                    &acknowledgement,
                    acknowledged_at + Duration::microseconds(1),
                )
                .await?
        );
        let expected_ownership = if index + 1 == rollback_dispatch.publications.len() {
            0
        } else {
            i64::try_from(members.len())?
        };
        assert_eq!(
            database
                .fetch_one_as(
                    sql_query::<i64>(
                        "select count(*) from gateway_route_ownership where gateway_rollout_id = ",
                    )
                    .bind(failed.rollout.id.as_uuid()),
                )
                .await?,
            expected_ownership,
            "failed Route ownership must release only after every rollback acknowledgement"
        );
        if index == 0 {
            assert_eq!(
                restarted
                    .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id,)
                    .await?
                    .state,
                GatewayRolloutRollbackState::Staged
            );
        }
        assert!(
            restarted
                .project_gateway_acknowledgement(
                    &acknowledgement,
                    acknowledged_at + Duration::microseconds(2),
                )
                .await?
        );
    }
    assert_eq!(
        restarted
            .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
            .await?
            .state,
        GatewayRolloutRollbackState::Succeeded
    );
    for (publication, certificate_id) in rollback_dispatch
        .publications
        .iter()
        .zip(reused_certificate_ids)
    {
        let active = restarted.active_routes(publication.node_id).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, retained.route_replicas[0].id);
        assert_eq!(active[0].gateway_revision, Some(publication.revision));
        assert_eq!(active[0].gateway_command_id, Some(publication.command_id));
        assert_eq!(
            active[0].snapshot_digest.as_deref(),
            Some(publication.snapshot_digest.as_str())
        );
        assert_eq!(
            restarted
                .find_gateway_certificate(
                    publication.node_id,
                    a3s_cloud_control_plane::modules::shared_kernel::domain::GatewayCertificateId::from_uuid(
                        certificate_id,
                    ),
                )
                .await?
                .state,
            GatewayCertificateState::Ready
        );
    }

    let retry = route_rollout_bundle(
        &restarted,
        fixture,
        claim,
        &scope,
        RouteId::new(),
        "failed-rollback.example.com",
        "postgres-exact-rollback-retry",
        now + Duration::seconds(12),
    )
    .await?;
    restarted.stage_gateway_rollout(retry).await?;
    Ok(())
}
