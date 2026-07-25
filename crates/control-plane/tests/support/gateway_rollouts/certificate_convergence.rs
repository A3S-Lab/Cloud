use super::*;
use a3s_cloud_control_plane::modules::edge::domain::repositories::GatewayCertificateConvergenceResult;
use a3s_cloud_control_plane::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use a3s_cloud_control_plane::modules::edge::{
    GatewayCertificateConvergenceState, GatewayCertificateReconciler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;

#[derive(Default)]
struct RecordingQueue;

#[async_trait]
impl IGatewayCommandQueue for RecordingQueue {
    async fn enqueue(
        &self,
        _publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        Ok(GatewayCommandDispatch { replayed: false })
    }
}

struct UnexpectedCertificateAuthority;

#[async_trait]
impl IGatewayCertificateAuthority for UnexpectedCertificateAuthority {
    async fn issue(
        &self,
        _request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        Err(GatewayCertificateAuthorityError::InvalidRequest(
            "route-less domain revocation must not issue a certificate".into(),
        ))
    }

    async fn revoke(
        &self,
        _certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(true)
    }
}

pub(super) async fn exercise_replicated_domain_revocation_targets(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut claim = verified_claim(
        repository,
        fixture,
        "replicated-convergence.example.net",
        now,
    )
    .await?;
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
                "postgres-replicated-certificate-convergence-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;
    let route_id = RouteId::new();
    let staged = repository
        .stage_gateway_rollout(
            route_rollout_bundle_with_validity(
                repository,
                fixture,
                &claim,
                &scope,
                route_id,
                claim.pattern.as_str(),
                "postgres-replicated-certificate-convergence-route",
                now + Duration::seconds(1),
                Duration::minutes(10),
            )
            .await?,
        )
        .await?;
    for publication in &staged.publications {
        let certificate = staged
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("replicated convergence rollout omitted a member certificate")?;
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

    assert!(repository
        .gateway_certificate_convergence_targets(
            now + Duration::seconds(4),
            now + Duration::seconds(4),
            100,
        )
        .await?
        .into_iter()
        .all(|target| !members.contains(&target.scope.node_id)));

    let edge: Arc<dyn IEdgeRepository> = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let reconciler = GatewayCertificateReconciler::new(
        edge,
        Arc::new(RecordingQueue),
        Arc::new(UnexpectedCertificateAuthority),
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })?,
        std::time::Duration::from_secs(1),
        Duration::days(1),
        Duration::hours(1),
        Duration::minutes(3),
        100,
    )?;
    let snapshot_renew_at = now + Duration::seconds(4);
    let snapshot_report = reconciler.run_once(snapshot_renew_at).await?;
    assert!(snapshot_report.convergence_targets >= members.len());
    assert!(snapshot_report.staged_convergences >= members.len());
    assert!(snapshot_report.dispatched_commands >= members.len());
    assert!(snapshot_report.failures.is_empty());
    let snapshot_renewals = repository
        .pending_gateway_certificate_convergences(100)
        .await?
        .into_iter()
        .filter(|result| members.contains(&result.convergence.node_id))
        .collect::<Vec<_>>();
    assert_eq!(snapshot_renewals.len(), members.len());
    assert!(snapshot_renewals.iter().all(|result| {
        result.convergence.reason
            == a3s_cloud_control_plane::modules::edge::GatewayCertificateConvergenceReason::SnapshotRenewal
            && result.convergence.retained_routes.len() == 1
            && result.convergence.rejected_routes.is_empty()
            && result.convergence.replacement_certificate_id.is_none()
            && result.certificate.is_none()
            && result.publication.certificate_request.is_none()
    }));
    for renewal in &snapshot_renewals {
        let acknowledged_at = snapshot_renew_at + Duration::seconds(1);
        assert!(
            repository
                .project_gateway_acknowledgement(
                    &acknowledgement(renewal, acknowledged_at),
                    acknowledged_at + Duration::microseconds(1),
                )
                .await?
        );
        let active = repository
            .active_routes(renewal.convergence.node_id)
            .await?;
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].gateway_certificate_id,
            Some(renewal.convergence.previous_certificate_id)
        );
        assert_eq!(
            active[0].gateway_revision,
            Some(renewal.publication.revision)
        );
        assert_eq!(
            repository
                .find_gateway_certificate(
                    renewal.convergence.node_id,
                    renewal.convergence.previous_certificate_id,
                )
                .await?
                .state,
            GatewayCertificateState::Ready,
        );
    }
    assert!(repository
        .obsolete_gateway_certificates(100)
        .await?
        .into_iter()
        .all(|certificate| !members.contains(&certificate.node_id)));

    let expected_version = claim.aggregate_version;
    claim.revoke(
        "replicated ownership removed",
        snapshot_renew_at + Duration::seconds(2),
    )?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: claim.clone(),
            expected_version,
            idempotency: IdempotencyRequest::new(
                format!("domain-claims/{}/revoke", claim.id),
                "postgres-replicated-certificate-convergence",
                b"replicated ownership removed",
            )?,
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
        })
        .await?;

    let targets = repository
        .gateway_certificate_convergence_targets(
            snapshot_renew_at + Duration::seconds(3),
            snapshot_renew_at + Duration::seconds(3),
            100,
        )
        .await?
        .into_iter()
        .filter(|target| members.contains(&target.scope.node_id))
        .collect::<Vec<_>>();
    assert_eq!(
        targets.len(),
        members.len(),
        "every replicated physical Gateway must independently converge revoked ownership"
    );
    assert!(targets.iter().all(|target| {
        target.routes.len() == 1
            && target.routes[0].route.id == route_id
            && target.routes[0].domain_claim_state
                == a3s_cloud_control_plane::modules::edge::DomainClaimState::Revoked
    }));

    let report = reconciler
        .run_once(snapshot_renew_at + Duration::seconds(3))
        .await?;
    assert!(report.convergence_targets >= members.len());
    assert!(report.staged_convergences >= members.len());
    assert!(report.dispatched_commands >= members.len());
    assert!(report.failures.is_empty());

    let restarted = PostgresEdgeRepository::new(executor.clone());
    let pending = restarted
        .pending_gateway_certificate_convergences(100)
        .await?
        .into_iter()
        .filter(|result| members.contains(&result.convergence.node_id))
        .collect::<Vec<_>>();
    assert_eq!(pending.len(), members.len());
    assert!(pending.iter().all(|result| {
        result.convergence.state == GatewayCertificateConvergenceState::Pending
            && result.convergence.retained_routes.is_empty()
            && result.convergence.rejected_routes.len() == 1
            && result.convergence.replacement_certificate_id.is_none()
            && result.certificate.is_none()
    }));
    let primary = convergence_for(&pending, scope.node_id)?;
    let secondary_id = members
        .iter()
        .copied()
        .find(|node_id| *node_id != scope.node_id)
        .ok_or("replicated convergence scope omitted a secondary")?;
    let secondary = convergence_for(&pending, secondary_id)?;

    let primary_acknowledged_at = snapshot_renew_at + Duration::seconds(4);
    let primary_ack = acknowledgement(primary, primary_acknowledged_at);
    assert!(
        restarted
            .project_gateway_acknowledgement(
                &primary_ack,
                primary_acknowledged_at + Duration::microseconds(1),
            )
            .await?
    );
    assert_eq!(ownership_count(database, route_id).await?, 1);
    assert_eq!(
        restarted
            .find_route(fixture.organization_id, route_id)
            .await?
            .state,
        RouteState::Active,
        "logical Route must remain active until every physical removal is acknowledged"
    );
    assert!(restarted.active_routes(scope.node_id).await?.is_empty());
    assert_eq!(restarted.active_routes(secondary_id).await?.len(), 1);
    assert!(
        restarted
            .project_gateway_acknowledgement(
                &primary_ack,
                primary_acknowledged_at + Duration::microseconds(2),
            )
            .await?
    );
    assert_eq!(ownership_count(database, route_id).await?, 1);

    let secondary_acknowledged_at = snapshot_renew_at + Duration::seconds(5);
    let secondary_ack = acknowledgement(secondary, secondary_acknowledged_at);
    assert!(
        restarted
            .project_gateway_acknowledgement(
                &secondary_ack,
                secondary_acknowledged_at + Duration::microseconds(1),
            )
            .await?
    );
    assert_eq!(ownership_count(database, route_id).await?, 0);
    let rejected_route = restarted
        .find_route(fixture.organization_id, route_id)
        .await?;
    assert_eq!(rejected_route.state, RouteState::Rejected);
    assert_eq!(
        rejected_route.failure.as_deref(),
        Some("domain ownership is no longer verified")
    );
    for node_id in members {
        assert!(restarted.active_routes(node_id).await?.is_empty());
    }
    assert!(
        PostgresEdgeRepository::new(executor.clone())
            .project_gateway_acknowledgement(
                &secondary_ack,
                secondary_acknowledged_at + Duration::microseconds(2),
            )
            .await?
    );

    let replacement_claim = verified_claim(
        &restarted,
        fixture,
        "replicated-convergence.example.net",
        snapshot_renew_at + Duration::seconds(6),
    )
    .await?;
    let replacement = route_rollout_bundle(
        &restarted,
        fixture,
        &replacement_claim,
        &scope,
        RouteId::new(),
        replacement_claim.pattern.as_str(),
        "postgres-replicated-certificate-convergence-republish",
        snapshot_renew_at + Duration::seconds(7),
    )
    .await?;
    let replacement = restarted.stage_gateway_rollout(replacement).await?;
    assert_eq!(replacement.route_replicas.len(), members.len());
    assert_eq!(
        ownership_count(database, replacement.route_replicas[0].id).await?,
        i64::try_from(members.len())?
    );
    Ok(())
}

fn convergence_for(
    pending: &[GatewayCertificateConvergenceResult],
    node_id: NodeId,
) -> Result<&GatewayCertificateConvergenceResult, Box<dyn std::error::Error>> {
    pending
        .iter()
        .find(|result| result.convergence.node_id == node_id)
        .ok_or_else(|| "replicated Gateway convergence member disappeared".into())
}

fn acknowledgement(
    convergence: &GatewayCertificateConvergenceResult,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: convergence.publication.command_id.as_uuid(),
        node_id: convergence.publication.node_id.as_uuid(),
        gateway_id: convergence.publication.node_id.as_uuid(),
        revision: convergence.publication.revision,
        snapshot_digest: convergence.publication.snapshot_digest.clone(),
        expires_at: convergence.publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}

async fn ownership_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    route_id: RouteId,
) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from gateway_route_ownership where route_id = ")
                .bind(route_id.as_uuid()),
        )
        .await?)
}
