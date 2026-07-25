use super::{
    PublishRoute, PublishRouteHandler, RevokeDomainClaim, RevokeDomainClaimHandler,
    SignGatewayCertificate, SignGatewayCertificateHandler, VerifyDomainClaim,
    VerifyDomainClaimHandler,
};
use crate::modules::edge::domain::events::{DomainClaimChanged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, IEdgeRepository, TransitionDomainClaim,
};
use crate::modules::edge::domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest,
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IDomainOwnershipVerifier, IGatewayCertificateAuthority, IGatewayCommandQueue,
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, DomainNamePattern, GatewayCertificate,
    GatewayCertificateMaterial, GatewayPublication, GatewayRolloutPolicy, GatewayScope,
    RoutePortName, RouteTarget, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::infrastructure::{
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{GatewayAckState, GatewayCertificateSigningRequest, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use sha2::Digest;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

mod gateway_scope_tests;

#[derive(Clone)]
struct FixedTargetReader {
    target: ResolvedRouteTarget,
}

#[async_trait]
impl IRouteTargetReader for FixedTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: chrono::DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        if revision_id != self.target.target.workload_revision_id {
            return Err(RepositoryError::NotFound);
        }
        Ok(self.target.clone())
    }
}

struct ReplicatedTargetReader {
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    observed_at: chrono::DateTime<Utc>,
    calls: AtomicUsize,
}

#[async_trait]
impl IRouteTargetReader for ReplicatedTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: chrono::DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        Err(RepositoryError::Conflict(
            "replicated target reader requires the complete desired membership".into(),
        ))
    }

    async fn resolve_healthy_target_set(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        member_node_ids: &[NodeId],
        _now: chrono::DateTime<Utc>,
    ) -> Result<ResolvedRouteTargetSet, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if revision_id != self.revision_id || port_name.as_str() != "http" {
            return Err(RepositoryError::NotFound);
        }
        ResolvedRouteTargetSet::new(
            member_node_ids,
            member_node_ids
                .iter()
                .enumerate()
                .map(|(index, node_id)| ResolvedRouteTarget {
                    workload_id: self.workload_id,
                    node_id: *node_id,
                    target: RouteTarget::new(
                        self.workload_id,
                        self.revision_id,
                        format!(
                            "workload:{}:revision:{}",
                            self.workload_id, self.revision_id
                        ),
                        1,
                        RoutePortName::parse("http").expect("port"),
                        UpstreamEndpoint::parse(format!(
                            "http://127.0.0.1:{}",
                            49_152 + u16::try_from(index).expect("member ordinal")
                        ))
                        .expect("upstream"),
                        self.observed_at,
                    )
                    .expect("Route target"),
                })
                .collect(),
        )
        .map_err(RepositoryError::Conflict)
    }
}

struct UnavailableTargetReader;

#[async_trait]
impl IRouteTargetReader for UnavailableTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: chrono::DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        Err(RepositoryError::Conflict(
            "current target evidence is no longer available".into(),
        ))
    }
}

#[derive(Default)]
struct RetryableDomainOwnershipVerifier {
    calls: AtomicUsize,
}

#[async_trait]
impl IDomainOwnershipVerifier for RetryableDomainOwnershipVerifier {
    async fn verify(
        &self,
        request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError> {
        assert_eq!(request.presented_proof, request.expected_value);
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(DomainOwnershipVerificationError::NotReady(
                "expected DNS TXT challenge is not observable".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct RecordingGatewayQueue {
    commands: Mutex<Vec<GatewayPublication>>,
}

struct RecordingGatewayCertificateAuthority {
    calls: AtomicUsize,
    unavailable: AtomicBool,
}

#[async_trait]
impl IGatewayCertificateAuthority for RecordingGatewayCertificateAuthority {
    async fn issue(
        &self,
        request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(GatewayCertificateAuthorityError::Unavailable(
                "test provider unavailable".into(),
            ));
        }
        Ok(GatewayCertificateMaterial {
            serial_number: request.certificate_id.to_string(),
            fingerprint: format!(
                "sha256:{:x}",
                sha2::Sha256::digest(request.csr_pem.as_bytes())
            ),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at: request.issued_at,
            expires_at: request.expires_at,
        })
    }

    async fn revoke(
        &self,
        _certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(!self.unavailable.load(Ordering::SeqCst))
    }
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        publication.snapshot().map_err(RepositoryError::Conflict)?;
        let mut commands = self.commands.lock().await;
        let replayed = commands
            .iter()
            .any(|existing| existing.command_id == publication.command_id);
        if !replayed {
            commands.push(publication.clone());
        }
        Ok(GatewayCommandDispatch { replayed })
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

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

#[allow(clippy::too_many_arguments)]
fn command(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    gateway_scope_id: GatewayScopeId,
    revision_id: WorkloadRevisionId,
    domain_claim_id: DomainClaimId,
    hostname: &str,
    key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> PublishRoute {
    PublishRoute {
        organization_id,
        project_id,
        environment_id,
        gateway_scope_id,
        workload_revision_id: revision_id,
        domain_claim_id,
        hostname: hostname.into(),
        path_prefix: "/v1".into(),
        port_name: "http".into(),
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
        requested_at,
    }
}

async fn gateway_scope(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    node_id: NodeId,
    now: chrono::DateTime<Utc>,
) -> GatewayScopeId {
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        node_id,
        now,
    )
    .expect("Gateway scope");
    edge.create_gateway_scope(CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: IdempotencyRequest::new(
            "test-gateway-scopes",
            scope.id.to_string(),
            scope.node_id.to_string().as_bytes(),
        )
        .expect("scope idempotency"),
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
    })
    .await
    .expect("create Gateway scope");
    scope.id
}

fn fixed_target(
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    node_id: NodeId,
) -> ResolvedRouteTarget {
    ResolvedRouteTarget {
        workload_id,
        node_id,
        target: RouteTarget::new(
            workload_id,
            revision_id,
            format!("workload:{workload_id}:revision:{revision_id}"),
            1,
            RoutePortName::parse("http").expect("port name"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            Utc::now() - Duration::days(1),
        )
        .expect("route target"),
    }
}

async fn verified_claim(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    pattern: &str,
    now: chrono::DateTime<Utc>,
) -> DomainClaimId {
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse(pattern).expect("pattern"),
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .expect("claim");
    let created = DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("created event");
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: claim.clone(),
        idempotency: IdempotencyRequest::new(
            "test-domain-claims",
            claim.id.to_string(),
            claim.pattern.as_str().as_bytes(),
        )
        .expect("create idempotency"),
        event: created,
    })
    .await
    .expect("create claim");
    let expected_version = claim.aggregate_version;
    claim
        .verify(now + Duration::milliseconds(1))
        .expect("verify claim");
    let verified = DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("verified event");
    edge.transition_domain_claim(TransitionDomainClaim {
        claim: claim.clone(),
        expected_version,
        idempotency: IdempotencyRequest::new(
            "test-domain-claim-verifications",
            claim.id.to_string(),
            b"verified",
        )
        .expect("verify idempotency"),
        event: verified,
    })
    .await
    .expect("persist verified claim");
    claim.id
}

async fn record_issued_certificate(
    edge: &Arc<InMemoryEdgeRepository>,
    certificate: &GatewayCertificate,
    now: chrono::DateTime<Utc>,
) {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            GatewayCertificateMaterial {
                serial_number: issued.id.to_string(),
                fingerprint: format!("sha256:{}", "a".repeat(64)),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at: now,
                expires_at: now + Duration::days(30),
            },
            now,
        )
        .expect("issue certificate");
    edge.transition_gateway_certificate(issued, expected_version)
        .await
        .expect("persist issued certificate");
}

async fn stage_certificate(
    routes: Arc<InMemoryEdgeRepository>,
    node_id: NodeId,
    hostname: &str,
    key: &str,
    now: chrono::DateTime<Utc>,
) -> crate::modules::edge::domain::repositories::EdgeRoutePublicationResult {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let domain_claim_id = verified_claim(
        &routes,
        organization_id,
        project_id,
        environment_id,
        hostname,
        now,
    )
    .await;
    let workload_id = WorkloadId::new();
    let gateway_scope_id = gateway_scope(
        &routes,
        organization_id,
        project_id,
        environment_id,
        node_id,
        now,
    )
    .await;
    PublishRouteHandler::new(
        routes,
        Arc::new(FixedTargetReader {
            target: fixed_target(workload_id, revision_id, node_id),
        }),
        Arc::new(RecordingGatewayQueue::default()),
        compiler(),
        Duration::minutes(3),
    )
    .expect("publish handler")
    .execute(
        command(
            organization_id,
            project_id,
            environment_id,
            gateway_scope_id,
            revision_id,
            domain_claim_id,
            hostname,
            key,
            now,
        ),
        context(),
    )
    .await
    .expect("command bus")
    .expect("stage Gateway certificate")
    .publication
}

#[tokio::test]
async fn unobserved_domain_proof_remains_pending_and_retryable_with_the_same_key() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let now = Utc::now();
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse("api.example.com").expect("domain pattern"),
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .expect("domain claim");
    let edge = Arc::new(InMemoryEdgeRepository::new());
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: claim.clone(),
        idempotency: IdempotencyRequest::new(
            "test-domain-claim-creation",
            claim.id.to_string(),
            claim.pattern.as_str().as_bytes(),
        )
        .expect("create idempotency"),
        event: DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("created event"),
    })
    .await
    .expect("persist domain claim");

    let verifier = Arc::new(RetryableDomainOwnershipVerifier::default());
    let repository: Arc<dyn IEdgeRepository> = edge.clone();
    let handler = VerifyDomainClaimHandler::new(repository, verifier.clone());
    let command = VerifyDomainClaim {
        organization_id,
        claim_id: claim.id,
        proof: claim.challenge_value.clone(),
        idempotency_key: "verify-api-domain".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(1),
    };

    let first_error = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect_err("DNS proof is not observable yet");
    assert!(matches!(first_error, ApplicationError::Conflict(_)));
    claim = edge
        .find_domain_claim(organization_id, claim.id)
        .await
        .expect("pending domain claim");
    assert_eq!(claim.state, DomainClaimState::Pending);
    assert_eq!(claim.aggregate_version, 1);
    assert_eq!(edge.outbox_events().await.len(), 1);

    let canonical = serde_json::to_vec(&serde_json::json!({
        "organization_id": command.organization_id,
        "claim_id": command.claim_id,
        "proof": command.proof,
    }))
    .expect("canonical verification request");
    let verification_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/domain-claims/{}/verify",
            command.organization_id, command.claim_id
        ),
        command.idempotency_key.clone(),
        &canonical,
    )
    .expect("verification idempotency");
    assert!(edge
        .replay_domain_claim_write(&verification_idempotency)
        .await
        .expect("verification replay lookup")
        .is_none());

    let verified = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect("retry domain verification");
    assert!(!verified.replayed);
    assert_eq!(verified.claim.state, DomainClaimState::Verified);
    assert_eq!(verified.claim.aggregate_version, 2);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 2);
    assert_eq!(edge.outbox_events().await.len(), 2);

    let replay = handler
        .execute(command, context())
        .await
        .expect("command bus")
        .expect("replay domain verification");
    assert!(replay.replayed);
    assert_eq!(replay.claim, verified.claim);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn revokes_a_verified_domain_claim_idempotently_with_a_bounded_reason() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let now = Utc::now();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "revoke.example.com",
        now,
    )
    .await;
    let repository: Arc<dyn IEdgeRepository> = edge.clone();
    let handler = RevokeDomainClaimHandler::new(repository);
    let command = RevokeDomainClaim {
        organization_id,
        claim_id,
        reason: "  customer request\nconfirmed  ".into(),
        idempotency_key: "revoke-domain".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(1),
    };

    let revoked = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect("revoke domain claim");
    assert!(!revoked.replayed);
    assert_eq!(revoked.claim.state, DomainClaimState::Revoked);
    assert_eq!(
        revoked.claim.failure.as_deref(),
        Some("customer request confirmed")
    );
    assert_eq!(
        edge.outbox_events()
            .await
            .last()
            .expect("revocation event")
            .event_key,
        "edge.domain-claim.revoked"
    );

    let replay = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect("replay revocation");
    assert!(replay.replayed);
    assert_eq!(replay.claim, revoked.claim);

    let conflict = handler
        .execute(
            RevokeDomainClaim {
                reason: "different reason".into(),
                ..command.clone()
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("same key must bind one reason");
    assert!(matches!(conflict, ApplicationError::Conflict(_)));

    let invalid = handler
        .execute(
            RevokeDomainClaim {
                idempotency_key: "invalid-reason".into(),
                reason: " \n ".into(),
                ..command
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("empty reason");
    assert!(matches!(invalid, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn publishes_one_exact_command_and_replays_the_same_route_intent() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let workload_id = WorkloadId::new();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let now = Utc::now();
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: fixed_target(workload_id, revision_id, node_id),
        }),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");
    let domain_claim_id = verified_claim(
        &routes,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
        now,
    )
    .await;
    let gateway_scope_id = gateway_scope(
        &routes,
        organization_id,
        project_id,
        environment_id,
        node_id,
        now,
    )
    .await;
    let request = command(
        organization_id,
        project_id,
        environment_id,
        gateway_scope_id,
        revision_id,
        domain_claim_id,
        "api.example.com",
        "publish-api",
        now,
    );
    let first = handler
        .execute(request.clone(), context())
        .await
        .expect("command bus")
        .expect("publish route");
    assert!(!first.publication.replayed);
    assert!(!first.command_replayed);
    assert_eq!(first.publication.publication.revision, 1);
    assert_eq!(
        first
            .publication
            .publication
            .acl
            .matches("routers \"")
            .count(),
        1
    );

    let original_correlation_id = request.request_id;
    let mut replay_request = request;
    replay_request.request_id = Uuid::now_v7();
    replay_request.requested_at += Duration::hours(1);
    assert_ne!(replay_request.request_id, original_correlation_id);
    let replay_handler = PublishRouteHandler::new(
        routes,
        Arc::new(UnavailableTargetReader),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("replay handler");
    let replay = replay_handler
        .execute(replay_request, context())
        .await
        .expect("command bus")
        .expect("replay route");
    assert!(replay.publication.replayed);
    assert!(replay.command_replayed);
    assert_eq!(replay.publication.route.id, first.publication.route.id);
    assert_eq!(
        replay.publication.publication.command_correlation_id,
        original_correlation_id
    );
    assert_eq!(queue.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn next_publication_contains_every_active_route_in_the_scope() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let workload_id = WorkloadId::new();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: fixed_target(workload_id, revision_id, node_id),
        }),
        queue,
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");
    let now = Utc::now();
    let domain_claim_id = verified_claim(
        &routes,
        organization_id,
        project_id,
        environment_id,
        "*.example.com",
        now,
    )
    .await;
    let gateway_scope_id = gateway_scope(
        &routes,
        organization_id,
        project_id,
        environment_id,
        node_id,
        now,
    )
    .await;
    let first = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                gateway_scope_id,
                revision_id,
                domain_claim_id,
                "api.example.com",
                "first",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("first route");
    record_issued_certificate(
        &routes,
        &first.publication.certificate,
        now + Duration::seconds(1),
    )
    .await;
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: first.publication.publication.command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        gateway_id: node_id.as_uuid(),
        revision: 1,
        snapshot_digest: first.publication.publication.snapshot_digest.clone(),
        expires_at: first.publication.publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    };
    assert!(routes
        .project_gateway_acknowledgement(
            &acknowledgement,
            acknowledgement.acknowledged_at + Duration::seconds(1),
        )
        .await
        .expect("project acknowledgement"));

    let second = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                gateway_scope_id,
                revision_id,
                domain_claim_id,
                "web.example.com",
                "second",
                now + Duration::seconds(2),
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("second route");
    assert_eq!(second.publication.publication.revision, 2);
    assert_eq!(
        second
            .publication
            .publication
            .acl
            .matches("routers \"")
            .count(),
        2
    );
    assert!(second
        .publication
        .publication
        .acl
        .contains("Host(`api.example.com`)"));
    assert!(second
        .publication
        .publication
        .acl
        .contains("Host(`web.example.com`)"));
}

mod gateway_certificate_tests;
