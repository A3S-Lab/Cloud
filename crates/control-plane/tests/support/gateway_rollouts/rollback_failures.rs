use super::*;

#[derive(Clone, Copy)]
enum RollbackFailure {
    Rejected,
    Unavailable,
}

impl RollbackFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Unavailable => "unavailable",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_failed_exact_rollbacks(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    rejected_members: [NodeId; 2],
    unavailable_members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    exercise_failed_exact_rollback(
        repository,
        database,
        executor,
        fixture,
        claim,
        rejected_members,
        RollbackFailure::Rejected,
        now,
    )
    .await?;
    exercise_failed_exact_rollback(
        repository,
        database,
        executor,
        fixture,
        claim,
        unavailable_members,
        RollbackFailure::Unavailable,
        now + Duration::minutes(10),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn exercise_failed_exact_rollback(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    members: [NodeId; 2],
    failure: RollbackFailure,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let label = failure.label();
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
                "postgres-failed-exact-rollback-scopes",
                format!("{label}-{}", scope.id),
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;

    let retained_hostname = format!("retained-{label}-rollback.example.com");
    let retained = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                RouteId::new(),
                &retained_hostname,
                &format!("postgres-{label}-rollback-retained"),
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
            .ok_or("retained failed-rollback rollout omitted a member certificate")?;
        issue_certificate(repository, certificate, now + Duration::seconds(2)).await?;
        assert!(
            repository
                .project_gateway_acknowledgement(
                    &route_acknowledgement(
                        publication,
                        GatewayAckState::Applied,
                        now + Duration::seconds(3),
                    ),
                    now + Duration::seconds(3) + Duration::microseconds(1),
                )
                .await?
        );
    }

    let failed_hostname = format!("failed-{label}-rollback.example.com");
    let failed_route_id = RouteId::new();
    let failed = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                claim,
                &scope,
                failed_route_id,
                &failed_hostname,
                &format!("postgres-{label}-rollback-failed"),
                now + Duration::seconds(4),
            )
            .await?,
        )
        .await?;
    let applied = &failed.publications[0];
    let applied_certificate = failed
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied.node_id)
        .ok_or("failed rollout omitted its applied member certificate")?;
    issue_certificate(repository, applied_certificate, now + Duration::seconds(5)).await?;
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    applied,
                    GatewayAckState::Applied,
                    now + Duration::seconds(6),
                ),
                now + Duration::seconds(6) + Duration::microseconds(1),
            )
            .await?
    );
    let rejected = &failed.publications[1];
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    rejected,
                    GatewayAckState::Rejected,
                    now + Duration::seconds(7),
                ),
                now + Duration::seconds(7) + Duration::microseconds(1),
            )
            .await?
    );
    assert_eq!(
        repository
            .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
            .await?
            .state,
        GatewayRolloutRollbackState::Required
    );

    let rollback_repository: Arc<dyn IEdgeRepository> =
        Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let reconciler = GatewayRolloutRollbackReconciler::new(
        rollback_repository,
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
    let report = reconciler.run_once(now + Duration::seconds(8)).await?;
    assert_eq!(report.required_rollbacks, 1);
    assert_eq!(report.staged_rollbacks, 1);
    assert!(report.failures.is_empty());

    let restarted = PostgresEdgeRepository::new(executor.clone());
    let staged_rollback = restarted
        .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
        .await?;
    assert_eq!(staged_rollback.state, GatewayRolloutRollbackState::Staged);
    let rollback_dispatch = restarted
        .pending_gateway_rollout_dispatches(100)
        .await?
        .into_iter()
        .find(|target| target.rollout.id == staged_rollback.rollback_rollout_id)
        .ok_or("failed exact rollback omitted its dispatch")?;
    assert_eq!(rollback_dispatch.publications.len(), members.len());
    let reused_certificates = rollback_dispatch
        .publications
        .iter()
        .map(|publication| {
            publication
                .certificate_request
                .as_ref()
                .map(|request| (publication.node_id, request.certificate_id))
                .ok_or("failed exact rollback omitted a reused certificate")
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
            "failed exact rollback must not issue a replacement certificate"
        );
    }

    let first = &rollback_dispatch.publications[0];
    let first_acknowledged_at = now + Duration::seconds(9);
    assert!(
        restarted
            .project_gateway_acknowledgement(
                &route_acknowledgement(first, GatewayAckState::Applied, first_acknowledged_at,),
                first_acknowledged_at + Duration::microseconds(1),
            )
            .await?
    );
    let second = &rollback_dispatch.publications[1];
    match failure {
        RollbackFailure::Rejected => {
            let rejected_at = now + Duration::seconds(10);
            assert!(
                restarted
                    .project_gateway_acknowledgement(
                        &route_acknowledgement(second, GatewayAckState::Rejected, rejected_at,),
                        rejected_at + Duration::microseconds(1),
                    )
                    .await?
            );
        }
        RollbackFailure::Unavailable => {
            let child = restarted
                .find_gateway_rollout(fixture.organization_id, rollback_dispatch.rollout.id)
                .await?;
            restarted
                .mark_gateway_rollout_replica_unavailable(
                    fixture.organization_id,
                    child.id,
                    second.node_id,
                    child.aggregate_version,
                    "Gateway exact rollback command expired before acknowledgement",
                    second.command_not_after + Duration::seconds(1),
                )
                .await?;
        }
    }

    let failed_rollback = restarted
        .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
        .await?;
    assert_eq!(failed_rollback.state, GatewayRolloutRollbackState::Diverged);
    assert!(failed_rollback.blocks_scope());
    assert_eq!(
        restarted
            .find_gateway_rollout(fixture.organization_id, rollback_dispatch.rollout.id)
            .await?
            .state,
        GatewayRolloutState::Degraded
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_ownership where gateway_rollout_id = ",
                )
                .bind(failed.rollout.id.as_uuid()),
            )
            .await?,
        i64::try_from(members.len())?,
        "failed rollback must retain every ambiguous physical Route ownership"
    );
    assert_eq!(
        restarted
            .find_route(fixture.organization_id, failed_route_id)
            .await?
            .state,
        RouteState::Rejected
    );
    for (node_id, certificate_id) in reused_certificates {
        assert_eq!(
            restarted
                .find_gateway_certificate(
                    node_id,
                    a3s_cloud_control_plane::modules::shared_kernel::domain::GatewayCertificateId::from_uuid(
                        certificate_id,
                    ),
                )
                .await?
                .state,
            GatewayCertificateState::Ready,
            "rollback failure must not fail a reused certificate"
        );
        let active = restarted.active_routes(node_id).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].hostname.as_str(), retained_hostname);
    }
    assert!(restarted
        .pending_gateway_rollout_rollbacks(100)
        .await?
        .into_iter()
        .all(|target| target.failed_rollout.id != failed.rollout.id));

    let blocked = route_rollout_bundle(
        &restarted,
        fixture,
        claim,
        &scope,
        RouteId::new(),
        &failed_hostname,
        &format!("postgres-{label}-rollback-blocked"),
        now + Duration::minutes(5),
    )
    .await?;
    assert!(matches!(
        restarted.stage_gateway_rollout(blocked).await,
        Err(a3s_cloud_control_plane::modules::shared_kernel::domain::RepositoryError::Conflict(_))
    ));
    assert_eq!(
        PostgresEdgeRepository::new(executor.clone())
            .find_gateway_rollout_rollback(fixture.organization_id, failed.rollout.id)
            .await?,
        failed_rollback
    );
    Ok(())
}
