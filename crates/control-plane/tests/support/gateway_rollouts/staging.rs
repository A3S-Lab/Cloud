use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_atomic_route_rollout_staging(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    route_members: [NodeId; 2],
    conflict_members: [NodeId; 2],
    rebind_members: [NodeId; 2],
    rollback_members: [NodeId; 2],
    rejected_rollback_members: [NodeId; 2],
    unavailable_rollback_members: [NodeId; 2],
    certificate_convergence_members: [NodeId; 2],
    rejected_convergence_members: [NodeId; 2],
    unavailable_convergence_members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let claim = verified_claim(repository, fixture, "*.example.com", now).await?;
    let route_scope = persisted_route_scope(
        repository,
        fixture,
        route_members,
        "postgres-route-rollout-scope",
        now,
    )
    .await?;
    let route_id = RouteId::new();
    let stage = route_rollout_bundle(
        repository,
        fixture,
        &claim,
        &route_scope,
        route_id,
        "rollout.example.com",
        "postgres-route-rollout",
        now + Duration::milliseconds(1),
    )
    .await?;
    let staged = repository.stage_gateway_rollout(stage.clone()).await?;
    let replayed = repository.stage_gateway_rollout(stage).await?;
    assert!(!staged.replayed);
    assert!(replayed.replayed);
    assert_eq!(replayed.route_replicas, staged.route_replicas);
    assert_eq!(replayed.publications, staged.publications);
    assert_eq!(replayed.certificates, staged.certificates);
    assert_eq!(staged.route_replicas.len(), route_members.len());
    assert!(staged
        .route_replicas
        .iter()
        .all(|route| route.id == route_id));
    let primary = staged
        .route_replicas
        .iter()
        .find(|route| route.gateway_node_id == route_scope.node_id)
        .ok_or("Route rollout omitted its primary projection")?;
    assert_eq!(
        repository
            .find_route(fixture.organization_id, route_id)
            .await?,
        *primary
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_projections where gateway_rollout_id = ",
                )
                .bind(staged.rollout.id.as_uuid()),
            )
            .await?,
        i64::try_from(route_members.len())?
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(distinct gateway_node_id) from gateway_route_projections where gateway_rollout_id = ",
                )
                .bind(staged.rollout.id.as_uuid()),
            )
            .await?,
        i64::try_from(route_members.len())?
    );

    let applied_node = route_members
        .iter()
        .copied()
        .find(|node_id| *node_id != route_scope.node_id)
        .ok_or("replicated Route scope has no secondary member")?;
    let applied_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id == applied_node)
        .ok_or("Route rollout omitted its secondary publication")?;
    let applied_certificate = staged
        .certificates
        .iter()
        .find(|certificate| certificate.node_id == applied_node)
        .ok_or("Route rollout omitted its secondary certificate")?;
    issue_certificate(repository, applied_certificate, now + Duration::seconds(1)).await?;
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    applied_publication,
                    GatewayAckState::Applied,
                    now + Duration::seconds(2),
                ),
                now + Duration::seconds(2) + Duration::microseconds(1),
            )
            .await?
    );
    let ready = repository
        .find_gateway_rollout(fixture.organization_id, staged.rollout.id)
        .await?;
    assert_eq!(ready.state, GatewayRolloutState::Ready);
    assert!(ready.serves_traffic()?);
    assert_eq!(
        repository
            .find_route(fixture.organization_id, route_id)
            .await?
            .state,
        RouteState::Active
    );
    let secondary_active = repository.active_routes(applied_node).await?;
    assert_eq!(secondary_active.len(), 1);
    assert_eq!(secondary_active[0].id, route_id);
    assert_eq!(secondary_active[0].gateway_node_id, applied_node);
    assert!(repository
        .active_routes(route_scope.node_id)
        .await?
        .is_empty());

    let rejected_publication = staged
        .publications
        .iter()
        .find(|publication| publication.node_id == route_scope.node_id)
        .ok_or("Route rollout omitted its primary publication")?;
    assert!(
        repository
            .project_gateway_acknowledgement(
                &route_acknowledgement(
                    rejected_publication,
                    GatewayAckState::Rejected,
                    now + Duration::seconds(3),
                ),
                now + Duration::seconds(3) + Duration::microseconds(1),
            )
            .await?
    );
    let degraded = repository
        .find_gateway_rollout(fixture.organization_id, staged.rollout.id)
        .await?;
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert!(degraded.serves_traffic()?);
    assert_eq!(
        repository
            .find_route(fixture.organization_id, route_id)
            .await?
            .state,
        RouteState::Active
    );
    assert!(repository
        .active_routes(route_scope.node_id)
        .await?
        .is_empty());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_projections where gateway_rollout_id = ",
                )
                .bind(staged.rollout.id.as_uuid())
                .append(" and state = 'active'"),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_projections where gateway_rollout_id = ",
                )
                .bind(staged.rollout.id.as_uuid())
                .append(" and state = 'rejected'"),
            )
            .await?,
        1
    );

    let conflict_scope = persisted_route_scope(
        repository,
        fixture,
        conflict_members,
        "postgres-route-rollout-conflict-scope",
        now + Duration::seconds(4),
    )
    .await?;
    let conflicting = route_rollout_bundle(
        repository,
        fixture,
        &claim,
        &conflict_scope,
        route_id,
        "other-rollout.example.com",
        "postgres-route-rollout-conflict",
        now + Duration::seconds(4) + Duration::milliseconds(1),
    )
    .await?;
    let conflicting_rollout_id = conflicting.rollout.id;
    let conflicting_certificate_ids = conflicting
        .certificates
        .iter()
        .map(|certificate| certificate.id)
        .collect::<Vec<_>>();
    let outbox_count = database
        .fetch_one_as(sql_query::<i64>("select count(*) from outbox_events"))
        .await?;
    let idempotency_count = database
        .fetch_one_as(sql_query::<i64>("select count(*) from idempotency_records"))
        .await?;

    assert!(repository.stage_gateway_rollout(conflicting).await.is_err());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from gateway_route_projections where gateway_rollout_id = ",
                )
                .bind(conflicting_rollout_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from gateway_rollouts where id = ")
                    .bind(conflicting_rollout_id.as_uuid()),
            )
            .await?,
        0
    );
    for node_id in conflict_members {
        assert_eq!(
            repository.gateway_scope(node_id).await?,
            a3s_cloud_control_plane::modules::edge::GatewayScopeState::empty(node_id)
        );
        assert_eq!(
            database
                .fetch_one_as(
                    sql_query::<i64>("select count(*) from gateway_publications where node_id = ")
                        .bind(node_id.as_uuid()),
                )
                .await?,
            0
        );
    }
    for certificate_id in conflicting_certificate_ids {
        assert_eq!(
            database
                .fetch_one_as(
                    sql_query::<i64>("select count(*) from gateway_certificates where id = ")
                        .bind(certificate_id.as_uuid()),
                )
                .await?,
            0
        );
    }
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>("select count(*) from outbox_events"))
            .await?,
        outbox_count
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>("select count(*) from idempotency_records"))
            .await?,
        idempotency_count
    );

    exercise_retained_route_rebinding(repository, fixture, &claim, rebind_members, now).await?;
    exercise_exact_route_rollback(
        repository,
        database,
        executor,
        fixture,
        &claim,
        rollback_members,
        now + Duration::seconds(20),
    )
    .await?;
    rollback_failures::exercise_failed_exact_rollbacks(
        repository,
        database,
        executor,
        fixture,
        &claim,
        rejected_rollback_members,
        unavailable_rollback_members,
        now + Duration::seconds(40),
    )
    .await?;
    certificate_convergence::exercise_replicated_domain_revocation_targets(
        repository,
        database,
        executor,
        fixture,
        certificate_convergence_members,
        now + Duration::minutes(30),
    )
    .await?;
    certificate_convergence_failures::exercise_replicated_convergence_failures(
        repository,
        database,
        executor,
        fixture,
        rejected_convergence_members,
        unavailable_convergence_members,
        now + Duration::minutes(31),
    )
    .await?;
    Ok(())
}
