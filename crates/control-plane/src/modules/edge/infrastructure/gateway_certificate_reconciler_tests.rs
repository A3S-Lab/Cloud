use super::gateway_certificate_reconciler::{
    deterministic_certificate_id, deterministic_command_id, GatewayCertificateReconciler,
};
use super::{GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata};
use crate::modules::edge::domain::events::{DomainClaimChanged, RoutePublicationStaged};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, IEdgeRepository, StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial,
    GatewayCertificateState, GatewayPublication, GatewayPublicationState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
struct RecordingGatewayQueue {
    fail: AtomicBool,
    publications: Mutex<Vec<GatewayPublication>>,
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Storage(
                "queue endpoint password=dispatch-secret\nfailed".into(),
            ));
        }
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
            "tests issue certificate material directly".into(),
        ))
    }

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        if self.fail_revoke.load(Ordering::SeqCst) {
            return Err(GatewayCertificateAuthorityError::Unavailable(
                "vault token=provider-secret\nunavailable".into(),
            ));
        }
        self.revoked_serials.lock().await.push(
            certificate
                .material
                .as_ref()
                .expect("issued certificate")
                .serial_number
                .clone(),
        );
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(!self.fail_revoke.load(Ordering::SeqCst))
    }
}

struct Fixture {
    repository: Arc<InMemoryEdgeRepository>,
    compiler: GatewaySnapshotCompiler,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    node_id: NodeId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            repository: Arc::new(InMemoryEdgeRepository::new()),
            compiler: compiler(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            node_id: NodeId::new(),
            workload_id: WorkloadId::new(),
            workload_revision_id: WorkloadRevisionId::new(),
        }
    }

    async fn verified_claim(&self, pattern: &str, now: chrono::DateTime<Utc>) -> DomainClaim {
        let mut claim = DomainClaim::create(
            DomainClaimId::new(),
            self.organization_id,
            self.project_id,
            self.environment_id,
            DomainNamePattern::parse(pattern).expect("domain pattern"),
            "a".repeat(32),
            now,
        )
        .expect("domain claim");
        let correlation_id = Uuid::now_v7();
        self.repository
            .create_domain_claim(CreateDomainClaimWrite {
                claim: claim.clone(),
                idempotency: IdempotencyRequest::new(
                    format!("domain-claims/{}", claim.id),
                    "create",
                    b"create",
                )
                .expect("create idempotency"),
                event: DomainClaimChanged::envelope(&claim, correlation_id).expect("created event"),
            })
            .await
            .expect("create claim");
        let expected_version = claim.aggregate_version;
        claim
            .verify(now + Duration::milliseconds(1))
            .expect("verify claim");
        self.repository
            .transition_domain_claim(TransitionDomainClaim {
                claim: claim.clone(),
                expected_version,
                idempotency: IdempotencyRequest::new(
                    format!("domain-claims/{}", claim.id),
                    "verify",
                    b"verify",
                )
                .expect("verify idempotency"),
                event: DomainClaimChanged::envelope(&claim, correlation_id)
                    .expect("verified event"),
            })
            .await
            .expect("persist verified claim");
        claim
    }

    async fn revoke_claim(
        &self,
        mut claim: DomainClaim,
        now: chrono::DateTime<Utc>,
    ) -> DomainClaim {
        let expected_version = claim.aggregate_version;
        claim
            .revoke("ownership removed", now)
            .expect("revoke claim");
        self.repository
            .transition_domain_claim(TransitionDomainClaim {
                claim: claim.clone(),
                expected_version,
                idempotency: IdempotencyRequest::new(
                    format!("domain-claims/{}", claim.id),
                    "revoke",
                    b"ownership removed",
                )
                .expect("revoke idempotency"),
                event: DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("revoked event"),
            })
            .await
            .expect("persist revoked claim");
        claim
    }

    async fn activate_route(
        &self,
        claim: &DomainClaim,
        hostname: &str,
        now: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
    ) -> (Route, GatewayCertificate) {
        let certificate_id = GatewayCertificateId::new();
        let mut route = Route::create(
            RouteId::new(),
            self.organization_id,
            self.project_id,
            self.environment_id,
            self.node_id,
            RouteHostname::parse(hostname).expect("hostname"),
            RoutePath::parse("/").expect("path"),
            claim.id,
            claim.pattern.clone(),
            certificate_id,
            self.workload_id,
            self.workload_revision_id,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            now,
        )
        .expect("route");
        let scope = self
            .repository
            .gateway_scope(self.node_id)
            .await
            .expect("scope");
        let revision = scope.next_revision().expect("next revision");
        let mut complete_routes = self
            .repository
            .active_routes(self.node_id)
            .await
            .expect("active routes");
        complete_routes.push(route.clone());
        let snapshot = self
            .compiler
            .compile(
                GatewaySnapshotMetadata::new(
                    self.node_id,
                    revision,
                    scope.installed_revision,
                    now,
                    now + Duration::hours(24),
                ),
                certificate_id,
                &complete_routes,
            )
            .expect("snapshot");
        let command_id = NodeCommandId::new();
        route
            .stage(revision, command_id, snapshot.snapshot_digest.clone(), now)
            .expect("stage route");
        let publication = GatewayPublication::stage(
            self.node_id,
            command_id,
            Uuid::now_v7(),
            snapshot,
            now,
            now + Duration::minutes(3),
        )
        .expect("publication");
        let mut domain_claim_ids = complete_routes
            .iter()
            .filter_map(|route| route.domain_claim_id)
            .collect::<Vec<_>>();
        domain_claim_ids.sort();
        domain_claim_ids.dedup();
        let certificate = GatewayCertificate::provision(
            certificate_id,
            self.organization_id,
            self.node_id,
            domain_claim_ids,
            revision,
            command_id,
            publication.snapshot_digest.clone(),
            publication
                .certificate_request
                .clone()
                .expect("certificate request"),
            now,
        )
        .expect("certificate");
        let event =
            RoutePublicationStaged::envelope(&route, &publication).expect("publication event");
        let staged = self
            .repository
            .stage_route_publication(StageRoutePublication {
                route,
                certificate,
                publication,
                expected_scope_version: scope.aggregate_version,
                idempotency: IdempotencyRequest::new(
                    format!("routes/{hostname}"),
                    revision.to_string(),
                    hostname.as_bytes(),
                )
                .expect("route idempotency"),
                event,
            })
            .await
            .expect("stage publication");
        issue_certificate(
            self.repository.as_ref(),
            &staged.certificate,
            now + Duration::milliseconds(1),
            expires_at,
        )
        .await;
        let acknowledgement = NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: staged.publication.command_id.as_uuid(),
            node_id: self.node_id.as_uuid(),
            gateway_id: self.node_id.as_uuid(),
            revision,
            snapshot_digest: staged.publication.snapshot_digest,
            expires_at: staged.publication.snapshot_expires_at,
            state: GatewayAckState::Applied,
            ready: true,
            message: None,
            acknowledged_at: now + Duration::milliseconds(2),
        };
        self.repository
            .project_gateway_acknowledgement(
                &acknowledgement,
                acknowledgement.acknowledged_at + Duration::milliseconds(1),
            )
            .await
            .expect("apply publication");
        (
            self.repository
                .find_route(self.organization_id, staged.route.id)
                .await
                .expect("active route"),
            self.repository
                .find_gateway_certificate(self.node_id, staged.certificate.id)
                .await
                .expect("ready certificate"),
        )
    }
}

fn compiler() -> GatewaySnapshotCompiler {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .expect("compiler")
}

fn reconciler(
    fixture: &Fixture,
    queue: Arc<RecordingGatewayQueue>,
    authority: Arc<RecordingGatewayCertificateAuthority>,
) -> GatewayCertificateReconciler {
    let repository: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    GatewayCertificateReconciler::new(
        repository,
        queue,
        authority,
        fixture.compiler.clone(),
        std::time::Duration::from_secs(60),
        Duration::days(7),
        Duration::hours(6),
        Duration::minutes(3),
        100,
    )
    .expect("reconciler")
}

async fn issue_certificate(
    repository: &InMemoryEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            GatewayCertificateMaterial {
                serial_number: issued.id.to_string(),
                fingerprint: format!("sha256:{}", "c".repeat(64)),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at,
                expires_at,
            },
            issued_at,
        )
        .expect("record issuance");
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await
        .expect("persist issuance");
}

async fn apply_convergence(
    repository: &InMemoryEdgeRepository,
    publication: &GatewayPublication,
    acknowledged_at: chrono::DateTime<Utc>,
) {
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
    };
    repository
        .project_gateway_acknowledgement(
            &acknowledgement,
            acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("apply convergence");
}

#[tokio::test]
async fn durable_convergence_is_redispatched_after_command_queue_failure() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("renew.example.com", base).await;
    fixture
        .activate_route(
            &claim,
            "renew.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    queue.fail.store(true, Ordering::SeqCst);
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue.clone(), authority);
    let run_at = base + Duration::days(24);

    let first = reconciler.run_once(run_at).await.expect("first cycle");
    assert_eq!(first.staged_convergences, 1);
    assert_eq!(first.dispatched_commands, 0);
    assert_eq!(first.failures.len(), 1);
    assert_eq!(first.failures[0].error, "Gateway command dispatch failed");
    assert!(!first.failures[0].error.contains("dispatch-secret"));

    let pending = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending convergence");
    assert_eq!(pending.len(), 1);
    let revision = pending[0].publication.revision;
    assert_eq!(
        pending[0].publication.command_id,
        deterministic_command_id(fixture.node_id, revision)
    );
    assert_eq!(
        pending[0].convergence.replacement_certificate_id,
        Some(deterministic_certificate_id(fixture.node_id, revision))
    );

    queue.fail.store(false, Ordering::SeqCst);
    let second = reconciler
        .run_once(run_at + Duration::seconds(1))
        .await
        .expect("retry cycle");
    assert_eq!(second.pending_convergences, 1);
    assert_eq!(second.staged_convergences, 0);
    assert_eq!(second.dispatched_commands, 1);
    assert!(second.failures.is_empty());
    assert_eq!(queue.publications.lock().await.len(), 1);
}

#[tokio::test]
async fn applied_snapshot_is_renewed_before_expiry_without_reissuing_its_certificate() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("snapshot.example.com", base).await;
    let (route, certificate) = fixture
        .activate_route(
            &claim,
            "snapshot.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let initial_revision = route.gateway_revision.expect("installed revision");
    let initial_digest = route.snapshot_digest.clone().expect("installed digest");
    let initial_certificate_version = certificate.aggregate_version;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue.clone(), authority);
    let run_at = base + Duration::hours(18) + Duration::seconds(2);

    let report = reconciler.run_once(run_at).await.expect("renew snapshot");
    assert_eq!(report.staged_convergences, 1);
    assert_eq!(report.dispatched_commands, 1);
    assert_eq!(report.obsolete_certificates, 0);
    assert!(report.failures.is_empty());
    let pending = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending snapshot renewal")
        .pop()
        .expect("snapshot renewal");
    assert_eq!(
        pending.convergence.reason,
        crate::modules::edge::domain::GatewayCertificateConvergenceReason::SnapshotRenewal
    );
    assert_eq!(pending.convergence.previous_certificate_id, certificate.id);
    assert!(pending.convergence.replacement_certificate_id.is_none());
    assert!(pending.certificate.is_none());
    assert!(pending.publication.certificate_request.is_none());
    assert_eq!(
        pending.publication.expected_revision,
        Some(initial_revision)
    );
    assert_eq!(pending.publication.snapshot_digest, initial_digest);
    assert_eq!(
        queue.publications.lock().await[0].acl,
        pending.publication.acl
    );
    assert_eq!(
        fixture
            .repository
            .find_route(fixture.organization_id, route.id)
            .await
            .expect("route before renewal acknowledgement")
            .gateway_revision,
        Some(initial_revision)
    );

    let rejected_at = run_at + Duration::milliseconds(1);
    let rejected = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: pending.publication.command_id.as_uuid(),
        node_id: pending.publication.node_id.as_uuid(),
        gateway_id: pending.publication.node_id.as_uuid(),
        revision: pending.publication.revision,
        snapshot_digest: pending.publication.snapshot_digest.clone(),
        expires_at: pending.publication.snapshot_expires_at,
        state: GatewayAckState::Rejected,
        ready: false,
        message: Some("renewal rejected".into()),
        acknowledged_at: rejected_at,
    };
    fixture
        .repository
        .project_gateway_acknowledgement(&rejected, rejected_at + Duration::milliseconds(1))
        .await
        .expect("reject renewal");
    assert_eq!(
        fixture
            .repository
            .find_route(fixture.organization_id, route.id)
            .await
            .expect("route after rejected renewal")
            .gateway_revision,
        Some(initial_revision)
    );
    assert_eq!(
        fixture
            .repository
            .gateway_scope(fixture.node_id)
            .await
            .expect("scope after rejected renewal")
            .installed_revision,
        Some(initial_revision)
    );

    let retry_at = run_at + Duration::seconds(1);
    let retry = reconciler
        .run_once(retry_at)
        .await
        .expect("retry snapshot renewal");
    assert_eq!(retry.staged_convergences, 1);
    let renewal = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending retry")
        .pop()
        .expect("snapshot renewal retry");
    assert_eq!(
        renewal.convergence.reason,
        crate::modules::edge::domain::GatewayCertificateConvergenceReason::SnapshotRenewal
    );
    assert_eq!(renewal.publication.snapshot_digest, initial_digest);
    assert!(renewal.publication.revision > pending.publication.revision);
    apply_convergence(
        fixture.repository.as_ref(),
        &renewal.publication,
        retry_at + Duration::milliseconds(1),
    )
    .await;
    let renewed_route = fixture
        .repository
        .find_route(fixture.organization_id, route.id)
        .await
        .expect("renewed route");
    assert_eq!(
        renewed_route.gateway_revision,
        Some(renewal.publication.revision)
    );
    assert_eq!(
        renewed_route.gateway_command_id,
        Some(renewal.publication.command_id)
    );
    assert_eq!(
        renewed_route.snapshot_digest.as_deref(),
        Some(initial_digest.as_str())
    );
    assert_eq!(renewed_route.gateway_certificate_id, Some(certificate.id));
    let retained_certificate = fixture
        .repository
        .find_gateway_certificate(fixture.node_id, certificate.id)
        .await
        .expect("retained certificate");
    assert_eq!(
        retained_certificate.aggregate_version,
        initial_certificate_version
    );
    assert_eq!(retained_certificate.state, GatewayCertificateState::Ready);

    let settled = reconciler
        .run_once(retry_at + Duration::seconds(1))
        .await
        .expect("settled snapshot");
    assert_eq!(settled.convergence_targets, 0);
    assert_eq!(settled.obsolete_certificates, 0);
    assert!(settled.failures.is_empty());
}

#[tokio::test]
async fn revoked_claim_routes_change_only_after_exact_convergence_acknowledgement() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let revoked_claim = fixture.verified_claim("api.example.com", base).await;
    let retained_claim = fixture.verified_claim("web.example.com", base).await;
    let (revoked_route, _) = fixture
        .activate_route(
            &revoked_claim,
            "api.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let (retained_route, _) = fixture
        .activate_route(
            &retained_claim,
            "web.example.com",
            base + Duration::seconds(2),
            base + Duration::days(30),
        )
        .await;
    fixture
        .revoke_claim(revoked_claim, base + Duration::seconds(3))
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority);
    let run_at = base + Duration::seconds(4);

    let report = reconciler.run_once(run_at).await.expect("reconcile");
    assert_eq!(report.staged_convergences, 1);
    assert!(report.failures.is_empty());
    let pending = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending convergence");
    assert_eq!(pending.len(), 1);
    let convergence = &pending[0];
    assert_eq!(
        convergence.convergence.reason,
        crate::modules::edge::domain::GatewayCertificateConvergenceReason::DomainRevocation
    );
    assert_eq!(
        convergence.convergence.rejected_routes[0].route_id,
        revoked_route.id
    );
    assert_eq!(
        convergence.convergence.retained_routes[0].route_id,
        retained_route.id
    );
    assert_eq!(
        convergence
            .certificate
            .as_ref()
            .expect("replacement certificate")
            .domain_claim_ids,
        vec![retained_claim.id]
    );
    assert_eq!(
        fixture
            .repository
            .find_route(fixture.organization_id, revoked_route.id)
            .await
            .expect("old revoked route")
            .state,
        RouteState::Active
    );

    let replacement = convergence
        .certificate
        .as_ref()
        .expect("replacement certificate");
    issue_certificate(
        fixture.repository.as_ref(),
        replacement,
        run_at + Duration::milliseconds(1),
        run_at + Duration::days(30),
    )
    .await;
    apply_convergence(
        fixture.repository.as_ref(),
        &convergence.publication,
        run_at + Duration::milliseconds(2),
    )
    .await;
    let rejected = fixture
        .repository
        .find_route(fixture.organization_id, revoked_route.id)
        .await
        .expect("rejected route");
    let retained = fixture
        .repository
        .find_route(fixture.organization_id, retained_route.id)
        .await
        .expect("retained route");
    assert_eq!(rejected.state, RouteState::Rejected);
    assert_eq!(
        rejected.failure.as_deref(),
        Some("domain ownership is no longer verified")
    );
    assert_eq!(retained.state, RouteState::Active);
    assert_eq!(
        retained.gateway_certificate_id,
        convergence.convergence.replacement_certificate_id
    );
}

#[tokio::test]
async fn obsolete_provider_revocation_is_sanitized_and_retryable() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("retry.example.com", base).await;
    let (_, previous) = fixture
        .activate_route(
            &claim,
            "retry.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority.clone());
    let run_at = base + Duration::days(24);
    reconciler.run_once(run_at).await.expect("stage renewal");
    let pending = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending renewal")
        .pop()
        .expect("renewal");
    issue_certificate(
        fixture.repository.as_ref(),
        pending
            .certificate
            .as_ref()
            .expect("replacement certificate"),
        run_at + Duration::milliseconds(1),
        run_at + Duration::days(30),
    )
    .await;
    apply_convergence(
        fixture.repository.as_ref(),
        &pending.publication,
        run_at + Duration::milliseconds(2),
    )
    .await;

    authority.fail_revoke.store(true, Ordering::SeqCst);
    let failed = reconciler
        .run_once(run_at + Duration::seconds(1))
        .await
        .expect("failed provider cycle");
    assert_eq!(failed.obsolete_certificates, 1);
    assert_eq!(failed.revoked_certificates, 0);
    assert_eq!(failed.failures.len(), 1);
    assert_eq!(
        failed.failures[0].error,
        "Gateway certificate authority is unavailable"
    );
    assert!(!failed.failures[0].error.contains("provider-secret"));
    assert_eq!(
        fixture
            .repository
            .find_gateway_certificate(fixture.node_id, previous.id)
            .await
            .expect("retryable old certificate")
            .state,
        GatewayCertificateState::Ready
    );

    authority.fail_revoke.store(false, Ordering::SeqCst);
    let applied = reconciler
        .run_once(run_at + Duration::seconds(2))
        .await
        .expect("successful provider cycle");
    assert_eq!(applied.revoked_certificates, 1);
    assert!(applied.failures.is_empty());
    assert_eq!(
        fixture
            .repository
            .find_gateway_certificate(fixture.node_id, previous.id)
            .await
            .expect("revoked old certificate")
            .state,
        GatewayCertificateState::Revoked
    );
    assert_eq!(
        authority.revoked_serials.lock().await.as_slice(),
        [previous
            .material
            .as_ref()
            .expect("previous material")
            .serial_number
            .clone()]
    );
}

#[test]
fn reconciler_configuration_is_closed() {
    let fixture = Fixture::new();
    let repository: Arc<dyn IEdgeRepository> = fixture.repository;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    assert!(GatewayCertificateReconciler::new(
        repository,
        queue,
        authority,
        fixture.compiler,
        std::time::Duration::ZERO,
        Duration::days(7),
        Duration::hours(6),
        Duration::minutes(3),
        100,
    )
    .is_err());
    assert_eq!(GatewayPublicationState::Pending.as_str(), "pending");
}
