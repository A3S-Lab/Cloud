use super::*;
use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, GatewayCertificateConvergenceResult, TransitionDomainClaim,
};
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use crate::modules::edge::infrastructure::GatewayCertificateReconciler;
use crate::modules::edge::GatewayCertificateMaterial;
use async_trait::async_trait;

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
            "route-less replicated convergence must not issue a certificate".into(),
        ))
    }

    async fn revoke(
        &self,
        _certificate: &crate::modules::edge::GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(true)
    }
}

#[tokio::test]
async fn replicated_domain_revocation_releases_each_physical_owner_only_after_its_ack() {
    let repository = Arc::new(InMemoryEdgeRepository::new());
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
    persist_scope(&repository, &scope, "replicated-domain-revocation").await;
    let route_id = RouteId::new();
    let staged = repository
        .stage_gateway_rollout(route_rollout_bundle(
            &scope,
            route_id,
            1,
            "revoked-replicated.example.net",
            "replicated-domain-revocation-route",
            now,
        ))
        .await
        .expect("stage replicated Route");
    let domain_claim_id = staged.route_replicas[0]
        .domain_claim_id
        .expect("replicated Route domain Claim");
    let mut domain_claim = crate::modules::edge::DomainClaim::create(
        domain_claim_id,
        organization_id,
        scope.project_id,
        scope.environment_id,
        DomainNamePattern::parse("revoked-replicated.example.net").expect("domain pattern"),
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .expect("domain Claim");
    repository
        .create_domain_claim(CreateDomainClaimWrite {
            claim: domain_claim.clone(),
            idempotency: IdempotencyRequest::new(
                "replicated-domain-revocation-claims",
                domain_claim.id.to_string(),
                b"revoked-replicated.example.net",
            )
            .expect("claim idempotency"),
            event: DomainClaimChanged::envelope(&domain_claim, Uuid::now_v7())
                .expect("claim event"),
        })
        .await
        .expect("persist domain Claim");
    let pending_claim_version = domain_claim.aggregate_version;
    domain_claim
        .verify(now + Duration::microseconds(1))
        .expect("verify domain Claim");
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: domain_claim.clone(),
            expected_version: pending_claim_version,
            idempotency: IdempotencyRequest::new(
                "replicated-domain-revocation-verifications",
                domain_claim.id.to_string(),
                b"verified",
            )
            .expect("verification idempotency"),
            event: DomainClaimChanged::envelope(&domain_claim, Uuid::now_v7())
                .expect("verification event"),
        })
        .await
        .expect("verify persisted domain Claim");
    for publication in &staged.publications {
        let certificate = staged
            .certificates
            .iter()
            .find(|certificate| certificate.node_id == publication.node_id)
            .expect("member certificate");
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
            .expect("activate replicated Route");
    }

    assert!(repository
        .gateway_certificate_convergence_targets(
            now + Duration::seconds(3),
            now + Duration::seconds(3),
            10,
        )
        .await
        .expect("fresh replicated convergence targets")
        .is_empty());

    let repository_port: Arc<dyn IEdgeRepository> = repository.clone();
    let reconciler = GatewayCertificateReconciler::new(
        repository_port,
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
        })
        .expect("snapshot compiler"),
        std::time::Duration::from_secs(1),
        Duration::days(1),
        Duration::hours(1),
        Duration::minutes(3),
        10,
    )
    .expect("certificate reconciler");
    let snapshot_renew_at = now + Duration::hours(23) + Duration::minutes(1);
    let snapshot_report = reconciler
        .run_once(snapshot_renew_at)
        .await
        .expect("run replicated snapshot renewal");
    assert_eq!(snapshot_report.convergence_targets, members.len());
    assert_eq!(snapshot_report.staged_convergences, members.len());
    assert!(snapshot_report.failures.is_empty());
    let snapshot_renewals = repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending replicated snapshot renewals");
    assert_eq!(snapshot_renewals.len(), members.len());
    assert!(snapshot_renewals.iter().all(|result| {
        result.convergence.retained_routes.len() == 1
            && result.convergence.rejected_routes.is_empty()
            && result.convergence.replacement_certificate_id.is_none()
            && result.certificate.is_none()
            && result.publication.certificate_request.is_none()
    }));
    for renewal in &snapshot_renewals {
        let acknowledged_at = snapshot_renew_at + Duration::seconds(1);
        repository
            .project_gateway_acknowledgement(
                &convergence_ack(renewal, acknowledged_at),
                acknowledged_at,
            )
            .await
            .expect("apply replicated snapshot renewal");
        let active = repository
            .active_routes(renewal.convergence.node_id)
            .await
            .expect("renewed active Route");
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].gateway_certificate_id,
            Some(renewal.convergence.previous_certificate_id)
        );
        assert_eq!(
            active[0].gateway_revision,
            Some(renewal.publication.revision)
        );
    }
    assert!(repository
        .obsolete_gateway_certificates(10)
        .await
        .expect("active reused certificates")
        .is_empty());

    let verified_claim_version = domain_claim.aggregate_version;
    domain_claim
        .revoke(
            "replicated ownership removed",
            snapshot_renew_at + Duration::seconds(2),
        )
        .expect("revoke domain Claim");
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: domain_claim.clone(),
            expected_version: verified_claim_version,
            idempotency: IdempotencyRequest::new(
                "replicated-domain-revocations",
                domain_claim.id.to_string(),
                b"replicated ownership removed",
            )
            .expect("revocation idempotency"),
            event: DomainClaimChanged::envelope(&domain_claim, Uuid::now_v7())
                .expect("revocation event"),
        })
        .await
        .expect("persist domain revocation");
    let revocation_at = snapshot_renew_at + Duration::seconds(3);
    let report = reconciler
        .run_once(revocation_at)
        .await
        .expect("run replicated domain revocation");
    assert_eq!(report.convergence_targets, members.len());
    assert_eq!(report.staged_convergences, members.len());
    assert!(report.failures.is_empty());
    let pending = repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending replicated revocations");
    assert_eq!(pending.len(), members.len());
    assert!(pending.iter().all(|result| {
        result.convergence.retained_routes.is_empty()
            && result.convergence.rejected_routes.len() == 1
            && result.convergence.replacement_certificate_id.is_none()
            && result.certificate.is_none()
    }));
    let primary = convergence_for(&pending, scope.node_id);
    let secondary_id = members
        .iter()
        .copied()
        .find(|node_id| *node_id != scope.node_id)
        .expect("secondary member");
    let secondary = convergence_for(&pending, secondary_id);

    let primary_ack = convergence_ack(primary, revocation_at + Duration::seconds(1));
    repository
        .project_gateway_acknowledgement(&primary_ack, revocation_at + Duration::seconds(1))
        .await
        .expect("apply primary convergence");
    assert_eq!(
        repository
            .gateway_route_owner(scope.node_id, "revoked-replicated.example.net", "/")
            .await,
        None
    );
    assert_eq!(
        repository
            .gateway_route_owner(secondary_id, "revoked-replicated.example.net", "/")
            .await,
        Some(route_id)
    );
    assert_eq!(
        repository
            .find_route(organization_id, route_id)
            .await
            .expect("logical Route after primary acknowledgement")
            .state,
        RouteState::Active
    );

    let secondary_ack = convergence_ack(secondary, revocation_at + Duration::seconds(2));
    repository
        .project_gateway_acknowledgement(&secondary_ack, revocation_at + Duration::seconds(2))
        .await
        .expect("apply secondary convergence");
    for node_id in members {
        assert_eq!(
            repository
                .gateway_route_owner(node_id, "revoked-replicated.example.net", "/")
                .await,
            None
        );
        assert!(repository
            .active_routes(node_id)
            .await
            .expect("active member Routes")
            .is_empty());
    }
    assert_eq!(
        repository
            .find_route(organization_id, route_id)
            .await
            .expect("rejected logical Route")
            .state,
        RouteState::Rejected
    );
}

fn convergence_for(
    pending: &[GatewayCertificateConvergenceResult],
    node_id: NodeId,
) -> &GatewayCertificateConvergenceResult {
    pending
        .iter()
        .find(|result| result.convergence.node_id == node_id)
        .expect("member convergence")
}

fn convergence_ack(
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
