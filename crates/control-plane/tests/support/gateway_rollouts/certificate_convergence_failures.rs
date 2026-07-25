use super::*;
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, IEdgeRepository,
};
use a3s_cloud_control_plane::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use a3s_cloud_control_plane::modules::edge::{
    GatewayCertificateConvergenceState, GatewayCertificateReconciler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;

const EXPIRED_FAILURE: &str =
    "Gateway certificate convergence command expired before acknowledgement";

#[derive(Debug, Clone, Copy)]
enum ConvergenceFailure {
    Rejected,
    Unavailable,
}

impl ConvergenceFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Unavailable => "unavailable",
        }
    }
}

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
            "route-less failed convergence must not issue a certificate".into(),
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

#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_replicated_convergence_failures(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    rejected_members: [NodeId; 2],
    unavailable_members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    exercise_failure(
        repository,
        database,
        executor,
        fixture,
        rejected_members,
        ConvergenceFailure::Rejected,
        now,
    )
    .await?;
    exercise_failure(
        repository,
        database,
        executor,
        fixture,
        unavailable_members,
        ConvergenceFailure::Unavailable,
        now + Duration::minutes(20),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn exercise_failure(
    repository: &PostgresEdgeRepository,
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
    fixture: &GatewayRolloutFixture,
    members: [NodeId; 2],
    failure: ConvergenceFailure,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let label = failure.label();
    let hostname = format!("{label}-replicated-convergence.example.net");
    let mut claim = verified_claim(repository, fixture, &hostname, now).await?;
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
                format!("postgres-{label}-convergence-scopes"),
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;
    let route_id = RouteId::new();
    let staged = repository
        .stage_gateway_rollout(
            route_rollout_bundle(
                repository,
                fixture,
                &claim,
                &scope,
                route_id,
                &hostname,
                &format!("postgres-{label}-convergence-route"),
                now + Duration::seconds(1),
            )
            .await?,
        )
        .await?;
    for publication in &staged.publications {
        let certificate = staged
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .ok_or("failed convergence rollout omitted a member certificate")?;
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
    let installed_revisions = staged
        .publications
        .iter()
        .map(|publication| (publication.node_id, publication.revision))
        .collect::<BTreeMap<_, _>>();
    let certificate_ids = staged
        .certificates
        .iter()
        .map(|certificate| (certificate.node_id, certificate.id))
        .collect::<BTreeMap<_, _>>();

    let claim_version = claim.aggregate_version;
    claim.revoke(
        format!("{label} replicated ownership removal"),
        now + Duration::seconds(4),
    )?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: claim.clone(),
            expected_version: claim_version,
            idempotency: IdempotencyRequest::new(
                format!("domain-claims/{}/revoke", claim.id),
                format!("postgres-{label}-convergence-revocation"),
                label.as_bytes(),
            )?,
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
        })
        .await?;

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
    let staged_at = now + Duration::seconds(5);
    let staged_report = reconciler.run_once(staged_at).await?;
    assert!(staged_report.staged_convergences >= members.len());
    let pending = member_pending(repository, members).await?;
    assert_eq!(pending.len(), members.len());
    assert!(pending.iter().all(|result| {
        result.convergence.retained_routes.is_empty()
            && result.convergence.rejected_routes.len() == 1
            && result.convergence.replacement_certificate_id.is_none()
            && result.certificate.is_none()
    }));

    let retry_at = match failure {
        ConvergenceFailure::Rejected => {
            for (offset, convergence) in pending.iter().enumerate() {
                let acknowledged_at = staged_at
                    + Duration::seconds(1)
                    + Duration::microseconds(i64::try_from(offset)?);
                assert!(
                    repository
                        .project_gateway_acknowledgement(
                            &terminal_acknowledgement(
                                convergence,
                                GatewayAckState::Rejected,
                                acknowledged_at,
                            ),
                            acknowledged_at + Duration::microseconds(1),
                        )
                        .await?
                );
            }
            let retry_at = staged_at + Duration::seconds(2);
            let retry_report = reconciler.run_once(retry_at).await?;
            assert!(retry_report.staged_convergences >= members.len());
            retry_at
        }
        ConvergenceFailure::Unavailable => {
            let expired_at = pending
                .iter()
                .map(|result| result.publication.command_not_after)
                .max()
                .ok_or("unavailable convergence omitted its deadline")?;
            let retry_report = reconciler.run_once(expired_at).await?;
            assert!(retry_report.unavailable_convergences >= members.len());
            assert!(retry_report.staged_convergences >= members.len());
            expired_at
        }
    };

    for convergence in &pending {
        let terminal = repository
            .find_gateway_certificate_convergence(
                convergence.convergence.node_id,
                convergence.convergence.gateway_revision,
            )
            .await?
            .ok_or("terminal failed convergence disappeared")?;
        let expected_state = match failure {
            ConvergenceFailure::Rejected => GatewayCertificateConvergenceState::Rejected,
            ConvergenceFailure::Unavailable => GatewayCertificateConvergenceState::Unavailable,
        };
        assert_eq!(terminal.state, expected_state);
        assert_eq!(
            terminal.failure.as_deref(),
            Some(match failure {
                ConvergenceFailure::Rejected => "Gateway rejected certificate convergence",
                ConvergenceFailure::Unavailable => EXPIRED_FAILURE,
            })
        );
    }
    let retries = member_pending(repository, members).await?;
    assert_eq!(retries.len(), members.len());
    assert!(retries.iter().all(|retry| {
        pending.iter().any(|previous| {
            previous.convergence.node_id == retry.convergence.node_id
                && previous.convergence.gateway_revision < retry.convergence.gateway_revision
                && previous.convergence.previous_certificate_id
                    == retry.convergence.previous_certificate_id
        })
    }));

    assert_eq!(ownership_count(database, route_id).await?, 2);
    assert_eq!(
        repository
            .find_route(fixture.organization_id, route_id)
            .await?
            .state,
        RouteState::Active
    );
    for node_id in members {
        let active = repository.active_routes(node_id).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].gateway_certificate_id,
            certificate_ids.get(&node_id).copied()
        );
        assert_eq!(
            repository.gateway_scope(node_id).await?.installed_revision,
            installed_revisions.get(&node_id).copied()
        );
        assert_eq!(
            repository
                .find_gateway_certificate(
                    node_id,
                    *certificate_ids
                        .get(&node_id)
                        .ok_or("member certificate identity disappeared")?,
                )
                .await?
                .state,
            GatewayCertificateState::Ready
        );
    }
    assert!(repository
        .obsolete_gateway_certificates(100)
        .await?
        .into_iter()
        .all(|certificate| !certificate_ids.values().any(|id| *id == certificate.id)));

    let replacement_claim = verified_claim(
        repository,
        fixture,
        &hostname,
        retry_at + Duration::seconds(1),
    )
    .await?;
    let blocked = route_rollout_bundle(
        repository,
        fixture,
        &replacement_claim,
        &scope,
        RouteId::new(),
        &hostname,
        &format!("postgres-{label}-convergence-blocked"),
        retry_at + Duration::seconds(2),
    )
    .await;
    match blocked {
        Ok(blocked) => assert!(repository.stage_gateway_rollout(blocked).await.is_err()),
        Err(error) => assert_eq!(
            error.to_string(),
            "Gateway route ownership is not unique within the scope"
        ),
    }

    let restarted = PostgresEdgeRepository::new(executor.clone());
    assert_eq!(ownership_count(database, route_id).await?, 2);
    assert_eq!(
        restarted
            .find_route(fixture.organization_id, route_id)
            .await?
            .state,
        RouteState::Active
    );
    assert_eq!(
        member_pending(&restarted, members).await?.len(),
        members.len()
    );
    Ok(())
}

async fn member_pending(
    repository: &PostgresEdgeRepository,
    members: [NodeId; 2],
) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError> {
    Ok(repository
        .pending_gateway_certificate_convergences(100)
        .await?
        .into_iter()
        .filter(|result| members.contains(&result.convergence.node_id))
        .collect())
}

fn terminal_acknowledgement(
    convergence: &GatewayCertificateConvergenceResult,
    state: GatewayAckState,
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
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected)
            .then(|| "Gateway rejected certificate convergence".into()),
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
