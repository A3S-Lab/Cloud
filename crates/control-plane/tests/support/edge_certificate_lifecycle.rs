use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use a3s_cloud_control_plane::modules::edge::domain::events::DomainClaimChanged;
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, IEdgeRepository, TransitionDomainClaim,
};
use a3s_cloud_control_plane::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::persistence::PostgresEdgeRepository;
use a3s_cloud_control_plane::modules::edge::{
    DomainClaim, GatewayCertificate, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateMaterial, GatewayCertificateReconciler,
    GatewayCertificateState, GatewayPublication, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, RouteState,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct GatewayCertificateLifecycleScenario {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub domain_claim: DomainClaim,
    pub started_at: chrono::DateTime<Utc>,
}

#[derive(Default)]
struct RecordingGatewayQueue {
    publications: Mutex<Vec<GatewayPublication>>,
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let mut publications = self.publications.lock().await;
        let replayed = publications
            .iter()
            .any(|existing| existing.command_id == publication.command_id);
        publications.push(publication.clone());
        Ok(GatewayCommandDispatch { replayed })
    }
}

#[derive(Default)]
struct RecordingGatewayCertificateAuthority {
    fail_revoke: AtomicBool,
    revoked_serials: Mutex<Vec<String>>,
}

#[async_trait]
impl IGatewayCertificateAuthority for RecordingGatewayCertificateAuthority {
    async fn issue(
        &self,
        _request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        Err(GatewayCertificateAuthorityError::Unavailable(
            "integration test issues certificate material directly".into(),
        ))
    }

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        if self.fail_revoke.load(Ordering::SeqCst) {
            return Err(GatewayCertificateAuthorityError::Unavailable(
                "vault token=postgres-provider-secret\nunavailable".into(),
            ));
        }
        self.revoked_serials.lock().await.push(
            certificate
                .material
                .as_ref()
                .ok_or_else(|| {
                    GatewayCertificateAuthorityError::InvalidRequest(
                        "certificate has no material".into(),
                    )
                })?
                .serial_number
                .clone(),
        );
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(!self.fail_revoke.load(Ordering::SeqCst))
    }
}

pub async fn exercise(
    executor: &PostgresExecutor,
    mut scenario: GatewayCertificateLifecycleScenario,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let edge: Arc<dyn IEdgeRepository> = repository.clone();
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = GatewayCertificateReconciler::new(
        edge,
        queue.clone(),
        authority.clone(),
        compiler()?,
        std::time::Duration::from_secs(60),
        Duration::days(7),
        Duration::minutes(3),
        100,
    )?;
    let before_renewal = repository.active_routes(scenario.node_id).await?;
    if before_renewal.is_empty() {
        return Err("certificate lifecycle requires active routes".into());
    }
    let previous_certificate_id = before_renewal[0]
        .gateway_certificate_id
        .ok_or("active route has no Gateway certificate")?;
    if before_renewal
        .iter()
        .any(|route| route.gateway_certificate_id != Some(previous_certificate_id))
    {
        return Err("active routes do not share the installed Gateway certificate".into());
    }
    let previous_certificate = repository
        .find_gateway_certificate(scenario.node_id, previous_certificate_id)
        .await?;
    let previous_serial = previous_certificate
        .material
        .as_ref()
        .ok_or("installed certificate has no material")?
        .serial_number
        .clone();
    let scope_before = repository.gateway_scope(scenario.node_id).await?;

    let first_report = reconciler.run_once(scenario.started_at).await?;
    assert_eq!(first_report.convergence_targets, 1);
    assert_eq!(first_report.staged_convergences, 1);
    assert!(first_report.failures.is_empty());
    let first = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        first.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        before_renewal
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );

    issue_certificate(
        repository.as_ref(),
        first
            .certificate
            .as_ref()
            .ok_or("renewal omitted replacement certificate")?,
        scenario.started_at + Duration::milliseconds(100),
    )
    .await?;
    let rejected = acknowledgement(
        &first,
        GatewayAckState::Rejected,
        scenario.started_at + Duration::milliseconds(200),
    );
    repository
        .project_gateway_acknowledgement(
            &rejected,
            rejected.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    assert_eq!(
        repository
            .find_gateway_certificate_convergence(scenario.node_id, first.publication.revision,)
            .await?
            .ok_or("rejected convergence disappeared")?
            .state,
        GatewayCertificateConvergenceState::Rejected
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        before_renewal
    );
    assert_eq!(
        repository
            .gateway_scope(scenario.node_id)
            .await?
            .installed_revision,
        scope_before.installed_revision
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );

    let retry_at = scenario.started_at + Duration::seconds(1);
    let retry_report = reconciler.run_once(retry_at).await?;
    assert_eq!(retry_report.staged_convergences, 1);
    let renewal = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        renewal.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    let replacement = renewal
        .certificate
        .as_ref()
        .ok_or("renewal retry omitted replacement certificate")?;
    issue_certificate(
        repository.as_ref(),
        replacement,
        retry_at + Duration::milliseconds(100),
    )
    .await?;
    let applied = acknowledgement(
        &renewal,
        GatewayAckState::Applied,
        retry_at + Duration::milliseconds(200),
    );
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let after_renewal = repository.active_routes(scenario.node_id).await?;
    assert_eq!(after_renewal.len(), before_renewal.len());
    assert!(after_renewal.iter().all(|route| {
        route.state == RouteState::Active
            && route.gateway_certificate_id == Some(replacement.id)
            && route.gateway_revision == Some(renewal.publication.revision)
    }));
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );

    authority.fail_revoke.store(true, Ordering::SeqCst);
    let failed_revocation = reconciler
        .run_once(scenario.started_at + Duration::seconds(2))
        .await?;
    assert_eq!(failed_revocation.obsolete_certificates, 1);
    assert_eq!(failed_revocation.revoked_certificates, 0);
    assert_eq!(
        failed_revocation.failures[0].error,
        "Gateway certificate authority is unavailable"
    );
    assert!(!failed_revocation.failures[0]
        .error
        .contains("postgres-provider-secret"));
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );
    authority.fail_revoke.store(false, Ordering::SeqCst);
    let revoked = reconciler
        .run_once(scenario.started_at + Duration::seconds(3))
        .await?;
    assert_eq!(revoked.revoked_certificates, 1);
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&previous_serial));

    let mut expected_rejected_route_ids = after_renewal
        .iter()
        .filter(|route| route.domain_claim_id == Some(scenario.domain_claim.id))
        .map(|route| route.id)
        .collect::<Vec<_>>();
    let mut expected_retained_route_ids = after_renewal
        .iter()
        .filter(|route| route.domain_claim_id != Some(scenario.domain_claim.id))
        .map(|route| route.id)
        .collect::<Vec<_>>();
    expected_rejected_route_ids.sort();
    expected_retained_route_ids.sort();
    assert!(!expected_rejected_route_ids.is_empty());
    assert!(!expected_retained_route_ids.is_empty());

    let expected_claim_version = scenario.domain_claim.aggregate_version;
    scenario.domain_claim.revoke(
        "integration ownership removed",
        scenario.started_at + Duration::seconds(4),
    )?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: scenario.domain_claim.clone(),
            expected_version: expected_claim_version,
            idempotency: IdempotencyRequest::new(
                format!("domain-claims/{}/revoke", scenario.domain_claim.id),
                "postgres-certificate-lifecycle",
                b"integration ownership removed",
            )?,
            event: DomainClaimChanged::envelope(&scenario.domain_claim, Uuid::now_v7())?,
        })
        .await?;
    let filtered_at = scenario.started_at + Duration::seconds(5);
    let filtered_report = reconciler.run_once(filtered_at).await?;
    assert_eq!(filtered_report.staged_convergences, 1);
    let filtered = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        filtered.convergence.reason,
        GatewayCertificateConvergenceReason::DomainRevocation
    );
    let mut rejected_route_ids = filtered
        .convergence
        .rejected_routes
        .iter()
        .map(|route| route.route_id)
        .collect::<Vec<_>>();
    let mut retained_route_ids = filtered
        .convergence
        .retained_routes
        .iter()
        .map(|route| route.route_id)
        .collect::<Vec<_>>();
    rejected_route_ids.sort();
    retained_route_ids.sort();
    assert_eq!(rejected_route_ids, expected_rejected_route_ids);
    assert_eq!(retained_route_ids, expected_retained_route_ids);
    let filtered_replacement = filtered
        .certificate
        .as_ref()
        .ok_or("filtered convergence omitted replacement certificate")?;
    assert_eq!(
        filtered.convergence.replacement_certificate_id,
        Some(filtered_replacement.id)
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        after_renewal
    );
    issue_certificate(
        repository.as_ref(),
        filtered_replacement,
        filtered_at + Duration::milliseconds(100),
    )
    .await?;
    let filtered_ack = acknowledgement(
        &filtered,
        GatewayAckState::Applied,
        filtered_at + Duration::milliseconds(200),
    );
    repository
        .project_gateway_acknowledgement(
            &filtered_ack,
            filtered_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let after_filter = repository.active_routes(scenario.node_id).await?;
    let mut active_route_ids = after_filter
        .iter()
        .map(|route| route.id)
        .collect::<Vec<_>>();
    active_route_ids.sort();
    assert_eq!(active_route_ids, expected_retained_route_ids);
    assert!(after_filter.iter().all(|route| {
        route.gateway_certificate_id == Some(filtered_replacement.id)
            && route.gateway_revision == Some(filtered.publication.revision)
    }));
    for route_id in expected_rejected_route_ids {
        let stored = repository
            .find_route(scenario.organization_id, route_id)
            .await?;
        assert_eq!(stored.state, RouteState::Rejected);
        assert_eq!(
            stored.failure.as_deref(),
            Some("domain ownership is no longer verified")
        );
    }

    let remaining_revoked_at = scenario.started_at + Duration::seconds(6);
    let mut remaining_claim_ids = after_filter
        .iter()
        .filter_map(|route| route.domain_claim_id)
        .collect::<Vec<_>>();
    remaining_claim_ids.sort();
    remaining_claim_ids.dedup();
    for claim_id in remaining_claim_ids {
        let mut claim = repository
            .find_domain_claim(scenario.organization_id, claim_id)
            .await?;
        let expected_version = claim.aggregate_version;
        let reason = "integration remaining ownership removed";
        claim.revoke(reason, remaining_revoked_at)?;
        repository
            .transition_domain_claim(TransitionDomainClaim {
                claim: claim.clone(),
                expected_version,
                idempotency: IdempotencyRequest::new(
                    format!("domain-claims/{claim_id}/revoke"),
                    format!("postgres-certificate-lifecycle-{claim_id}"),
                    reason.as_bytes(),
                )?,
                event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
            })
            .await?;
    }
    let route_less_at = scenario.started_at + Duration::seconds(7);
    let route_less_report = reconciler.run_once(route_less_at).await?;
    assert_eq!(route_less_report.staged_convergences, 1);
    assert_eq!(route_less_report.revoked_certificates, 1);
    let route_less = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        route_less.convergence.reason,
        GatewayCertificateConvergenceReason::DomainRevocation
    );
    assert!(route_less.convergence.retained_routes.is_empty());
    assert_eq!(
        route_less.convergence.rejected_routes.len(),
        after_filter.len()
    );
    assert_eq!(route_less.convergence.replacement_certificate_id, None);
    assert_eq!(route_less.certificate, None);
    assert_eq!(route_less.publication.certificate_request, None);
    assert!(!route_less.publication.acl.contains("routers \""));
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        after_filter
    );
    let route_less_ack = acknowledgement(
        &route_less,
        GatewayAckState::Applied,
        route_less_at + Duration::milliseconds(100),
    );
    repository
        .project_gateway_acknowledgement(
            &route_less_ack,
            route_less_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    assert!(repository.active_routes(scenario.node_id).await?.is_empty());
    for route in after_filter {
        let stored = repository
            .find_route(scenario.organization_id, route.id)
            .await?;
        assert_eq!(stored.state, RouteState::Rejected);
        assert_eq!(
            stored.failure.as_deref(),
            Some("domain ownership is no longer verified")
        );
    }
    assert_eq!(
        repository
            .gateway_scope(scenario.node_id)
            .await?
            .installed_revision,
        Some(route_less.publication.revision)
    );

    let replacement_serial = replacement.id.to_string();
    let filtered_replacement_serial = filtered_replacement.id.to_string();
    let final_revocation = reconciler
        .run_once(scenario.started_at + Duration::seconds(8))
        .await?;
    assert_eq!(final_revocation.revoked_certificates, 1);
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, replacement.id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, filtered_replacement.id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&replacement_serial));
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&filtered_replacement_serial));
    assert!(!queue.publications.lock().await.is_empty());
    Ok(())
}

fn compiler() -> Result<GatewaySnapshotCompiler, String> {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
}

async fn pending_for(
    repository: &PostgresEdgeRepository,
    node_id: NodeId,
) -> Result<GatewayCertificateConvergenceResult, Box<dyn std::error::Error>> {
    repository
        .pending_gateway_certificate_convergences(100)
        .await?
        .into_iter()
        .find(|result| result.convergence.node_id == node_id)
        .ok_or_else(|| "Gateway certificate convergence was not pending".into())
}

async fn issue_certificate(
    repository: &PostgresEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued.record_issued(
        format!("sha256:{}", "d".repeat(64)),
        GatewayCertificateMaterial {
            serial_number: issued.id.to_string(),
            fingerprint: format!("sha256:{}", "e".repeat(64)),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at,
            expires_at: issued_at + Duration::days(30),
        },
        issued_at,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

fn acknowledgement(
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
        message: (state == GatewayAckState::Rejected).then(|| "reload rejected".into()),
        acknowledged_at,
    }
}
