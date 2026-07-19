use super::{
    PublishRoute, PublishRouteHandler, SignGatewayCertificate, SignGatewayCertificateHandler,
};
use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, IEdgeRepository, TransitionDomainClaim,
};
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue, IRouteTargetReader, RouteTarget,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial,
    GatewayPublication, RoutePortName, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::infrastructure::{
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, IdempotencyRequest, NodeId, OrganizationId, ProjectId,
    RepositoryError, WorkloadId, WorkloadRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{GatewayAckState, GatewayCertificateSigningRequest, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use sha2::Digest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
struct FixedTargetReader {
    target: RouteTarget,
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
    ) -> Result<RouteTarget, RepositoryError> {
        if revision_id != self.target.workload_revision_id {
            return Err(RepositoryError::NotFound);
        }
        Ok(self.target.clone())
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
    ) -> Result<RouteTarget, RepositoryError> {
        Err(RepositoryError::Conflict(
            "current target evidence is no longer available".into(),
        ))
    }
}

#[derive(Default)]
struct RecordingGatewayQueue {
    commands: Mutex<Vec<GatewayPublication>>,
}

struct RecordingGatewayCertificateAuthority {
    calls: AtomicUsize,
    unavailable: bool,
}

#[async_trait]
impl IGatewayCertificateAuthority for RecordingGatewayCertificateAuthority {
    async fn issue(
        &self,
        request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable {
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
        Ok(!self.unavailable)
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
    PublishRouteHandler::new(
        routes,
        Arc::new(FixedTargetReader {
            target: RouteTarget {
                workload_id: WorkloadId::new(),
                workload_revision_id: revision_id,
                node_id,
                upstream: UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            },
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
async fn publishes_one_exact_command_and_replays_the_same_route_intent() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: RouteTarget {
                workload_id: WorkloadId::new(),
                workload_revision_id: revision_id,
                node_id,
                upstream: UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            },
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
        Utc::now(),
    )
    .await;
    let request = command(
        organization_id,
        project_id,
        environment_id,
        revision_id,
        domain_claim_id,
        "api.example.com",
        "publish-api",
        Utc::now(),
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
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: RouteTarget {
                workload_id: WorkloadId::new(),
                workload_revision_id: revision_id,
                node_id,
                upstream: UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            },
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
    let first = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
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
        revision: 1,
        snapshot_digest: first.publication.publication.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
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

#[tokio::test]
async fn signs_each_gateway_certificate_once_for_the_authenticated_node_and_csr() {
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let node_id = NodeId::new();
    let now = Utc::now();
    let staged = stage_certificate(
        Arc::clone(&routes),
        node_id,
        "api.example.com",
        "sign-certificate",
        now,
    )
    .await;
    let authority = Arc::new(RecordingGatewayCertificateAuthority {
        calls: AtomicUsize::new(0),
        unavailable: false,
    });
    let repository: Arc<dyn IEdgeRepository> = routes.clone();
    let certificate_authority: Arc<dyn IGatewayCertificateAuthority> = authority.clone();
    let handler =
        SignGatewayCertificateHandler::new(repository, certificate_authority, Duration::days(30))
            .expect("signing handler");
    let request = GatewayCertificateSigningRequest {
        schema: GatewayCertificateSigningRequest::SCHEMA.into(),
        certificate_id: staged.certificate.id.as_uuid(),
        node_id: node_id.as_uuid(),
        csr_pem:
            "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n"
                .into(),
        requested_at: now + Duration::milliseconds(1),
    };
    let command = SignGatewayCertificate {
        authenticated_node_id: node_id,
        request: request.clone(),
        received_at: now + Duration::seconds(1),
    };
    let issued = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect("issue certificate");
    assert_eq!(issued.dns_names, vec!["api.example.com"]);
    assert!(!issued.certificate_pem.contains("PRIVATE KEY"));
    let replay = handler
        .execute(
            SignGatewayCertificate {
                received_at: now + Duration::seconds(2),
                ..command
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect("replay certificate");
    assert_eq!(replay, issued);
    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);

    let conflicting = handler
        .execute(
            SignGatewayCertificate {
                authenticated_node_id: node_id,
                request: GatewayCertificateSigningRequest {
                    csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nY29uZmxpY3Q=\n-----END CERTIFICATE REQUEST-----\n".into(),
                    ..request
                },
                received_at: now + Duration::seconds(3),
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("different CSR must conflict");
    assert!(matches!(
        conflicting,
        crate::modules::shared_kernel::application::ApplicationError::Conflict(_)
    ));
    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn records_gateway_certificate_authority_failure_without_provider_details() {
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let node_id = NodeId::new();
    let now = Utc::now();
    let staged = stage_certificate(
        Arc::clone(&routes),
        node_id,
        "failed.example.com",
        "failed-signing",
        now,
    )
    .await;
    let repository: Arc<dyn IEdgeRepository> = routes.clone();
    let handler = SignGatewayCertificateHandler::new(
        repository,
        Arc::new(RecordingGatewayCertificateAuthority {
            calls: AtomicUsize::new(0),
            unavailable: true,
        }),
        Duration::days(30),
    )
    .expect("signing handler");
    let result = handler
        .execute(
            SignGatewayCertificate {
                authenticated_node_id: node_id,
                request: GatewayCertificateSigningRequest {
                    schema: GatewayCertificateSigningRequest::SCHEMA.into(),
                    certificate_id: staged.certificate.id.as_uuid(),
                    node_id: node_id.as_uuid(),
                    csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nZmFpbA==\n-----END CERTIFICATE REQUEST-----\n".into(),
                    requested_at: now + Duration::milliseconds(1),
                },
                received_at: now + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("unavailable authority");
    assert!(matches!(
        result,
        crate::modules::shared_kernel::application::ApplicationError::Internal(_)
    ));
    let stored = routes
        .find_gateway_certificate(node_id, staged.certificate.id)
        .await
        .expect("failed certificate");
    assert_eq!(
        stored.state,
        crate::modules::edge::domain::GatewayCertificateState::Failed
    );
    assert_eq!(
        stored.failure.as_deref(),
        Some("Gateway certificate authority was unavailable")
    );
    assert!(!stored
        .failure
        .as_deref()
        .unwrap_or_default()
        .contains("test provider"));
}
