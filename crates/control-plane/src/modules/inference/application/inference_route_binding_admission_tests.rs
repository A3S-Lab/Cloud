//! First-principles fail-closed EdgeRouteBinding admission (I0.2b brick).

use crate::modules::edge::domain::events::{DomainClaimChanged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, IEdgeRepository, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayScope,
};
use crate::modules::edge::infrastructure::EdgeInferenceRouteBindingAdmissionAdapter;
use crate::modules::edge::InMemoryEdgeRepository;
use crate::modules::inference::application::{
    InferenceEdgeRouteBindingAdmissionRequest, IInferenceEdgeRouteBindingAdmissionPort,
    IInferenceRouteAclProjectionPort, PermitInferenceGrantCredentialAdmission,
    PublishInferenceRoute, PublishInferenceRouteHandler, ReviseInferenceRoute,
    ReviseInferenceRouteHandler, EDGE_ROUTE_BINDING_INVALID,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::infrastructure::InMemoryInferenceRouteRepository;
use crate::modules::inference::EmptyInferenceRouteAclProjectionPort;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite, NodeId,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceTargetAclProjection,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

struct AlwaysPresentEnvironmentRepository;

#[async_trait]
impl IEnvironmentRepository for AlwaysPresentEnvironmentRepository {
    async fn create(
        &self,
        environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        Ok(IdempotentWrite {
            value: environment,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        Ok(Some(Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("default").expect("environment name"),
            Utc::now(),
        )))
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Ok(Vec::new())
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn sample_model() -> InferenceModelAclProjection {
    InferenceModelAclProjection {
        alias: "chat-model".into(),
        model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
        targets: vec![InferenceTargetAclProjection {
            target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
            service: "model-service".into(),
            upstream_model: "internal/model-v1".into(),
            priority: 0,
            weight: 100,
        }],
    }
}

fn sample_grant() -> InferenceGrantAclProjection {
    InferenceGrantAclProjection {
        credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        credential_generation: 3,
        models: vec!["chat-model".into()],
        endpoints: vec![
            InferenceEndpointAcl::Models,
            InferenceEndpointAcl::ChatCompletions,
        ],
        limits: InferenceLimitsAclProjection {
            max_concurrent_requests: 2,
            requests_per_minute: 60,
            request_burst: 2,
            tokens_per_minute: 10_000,
        },
    }
}

fn binding(
    domain_claim_id: DomainClaimId,
    gateway_scope_id: GatewayScopeId,
    hostname: &str,
    path_prefix: &str,
) -> EdgeRouteBindingRef {
    EdgeRouteBindingRef::new(domain_claim_id, gateway_scope_id, hostname, path_prefix, 1).unwrap()
}

async fn verified_claim(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    pattern: &str,
) -> DomainClaimId {
    let now = Utc::now();
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

async fn gateway_scope(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> GatewayScopeId {
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        NodeId::new(),
        Utc::now(),
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

fn publish_handler(
    edge: Arc<InMemoryEdgeRepository>,
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> PublishInferenceRouteHandler {
    PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(EdgeInferenceRouteBindingAdmissionAdapter::new(edge)),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    )
}

fn revise_handler(
    edge: Arc<InMemoryEdgeRepository>,
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> ReviseInferenceRouteHandler {
    ReviseInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(EdgeInferenceRouteBindingAdmissionAdapter::new(edge)),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    )
}

fn assert_binding_invalid(error: ApplicationError) {
    match error {
        ApplicationError::Invalid(message) => {
            assert!(
                message.starts_with(EDGE_ROUTE_BINDING_INVALID),
                "expected {EDGE_ROUTE_BINDING_INVALID} prefix, got {message}"
            );
        }
        other => panic!("expected ApplicationError::Invalid, got {other:?}"),
    }
}

#[tokio::test]
async fn valid_binding_publishes_and_edge_invents_no_catalog() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let handler = publish_handler(edge, routes);
    let route = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![sample_grant()],
                binding: binding(claim_id, scope_id, "api.example.com", "/v1"),
                idempotency_key: "valid-binding".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.organization_id, organization_id);
    assert_eq!(route.binding().domain_claim_id, claim_id);

    let empty = EmptyInferenceRouteAclProjectionPort;
    assert!(empty
        .list_inference_route_acl_projections(&[])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn wrong_environment_claim_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        other_environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, scope_id, "api.example.com", "/v1"),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn unknown_claim_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(
                DomainClaimId::new(),
                scope_id,
                "api.example.com",
                "/v1",
            ),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn mismatched_hostname_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, scope_id, "other.example.com", "/v1"),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn wrong_environment_gateway_scope_is_rejected_when_available() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, other_environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, scope_id, "api.example.com", "/v1"),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn publish_handler_rejects_unknown_claim_before_persist() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let handler = publish_handler(edge, routes);
    let error = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![sample_grant()],
                binding: binding(
                    DomainClaimId::new(),
                    scope_id,
                    "api.example.com",
                    "/v1",
                ),
                idempotency_key: "unknown-claim".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_binding_invalid(error);
}

async fn pending_claim(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    pattern: &str,
) -> DomainClaimId {
    let now = Utc::now();
    let claim = DomainClaim::create(
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
    claim.id
}

#[tokio::test]
async fn pending_domain_claim_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = pending_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, scope_id, "api.example.com", "/v1"),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn zero_binding_generation_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let mut invalid = binding(claim_id, scope_id, "api.example.com", "/v1");
    invalid.binding_generation = 0;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            invalid,
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn empty_path_prefix_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let mut invalid = binding(claim_id, scope_id, "api.example.com", "/v1");
    invalid.path_prefix.clear();
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            invalid,
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn cross_organization_claim_is_rejected() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let other_organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        other_organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    let error = admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, scope_id, "api.example.com", "/v1"),
        ))
        .await
        .unwrap_err();
    assert_binding_invalid(error);
}

#[tokio::test]
async fn missing_gateway_scope_does_not_invent_membership_and_still_admits() {
    // Honesty: when Edge has no GatewayScope row, admission does not invent
    // membership; a verified same-environment DomainClaim is still required.
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let missing_scope_id = GatewayScopeId::new();
    let admission = EdgeInferenceRouteBindingAdmissionAdapter::new(edge);
    admission
        .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
            organization_id,
            project_id,
            environment_id,
            binding(claim_id, missing_scope_id, "api.example.com", "/v1"),
        ))
        .await
        .expect("missing scope must not invent membership or reject a verified claim");
}

#[tokio::test]
async fn revise_also_enforces_edge_binding_admission() {
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = verified_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await;
    let scope_id = gateway_scope(&edge, organization_id, project_id, environment_id).await;
    let publish = publish_handler(Arc::clone(&edge), Arc::clone(&routes));
    let published = publish
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![sample_grant()],
                binding: binding(claim_id, scope_id, "api.example.com", "/v1"),
                idempotency_key: "publish-before-revise-binding".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let pending_claim_id = pending_claim(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "pending.example.com",
    )
    .await;
    let revise = revise_handler(edge, routes);
    let error = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![sample_grant()],
                binding: binding(pending_claim_id, scope_id, "pending.example.com", "/v1"),
                idempotency_key: "revise-pending-binding".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_binding_invalid(error);
}

