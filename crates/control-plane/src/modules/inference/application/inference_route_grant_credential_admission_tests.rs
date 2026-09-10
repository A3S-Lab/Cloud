//! First-principles fail-closed grant→credential admission (I0.2b brick).

use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
use crate::modules::identity::IdentityInferenceGrantCredentialAdmissionAdapter;
use crate::modules::inference::application::{
    InferenceGrantCredentialAdmissionRequest, IInferenceGrantCredentialAdmissionPort,
    PermitInferenceEdgeRouteBindingAdmission, PublishInferenceRoute,
    PublishInferenceRouteHandler, ReviseInferenceRoute, ReviseInferenceRouteHandler,
    INFERENCE_GRANT_CREDENTIAL_INVALID,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::infrastructure::InMemoryInferenceRouteRepository;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite,
    InferenceCredentialId, OrganizationId, ProjectId, RepositoryError,
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

const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

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

fn grant_for(credential: &InferenceCredential) -> InferenceGrantAclProjection {
    InferenceGrantAclProjection {
        credential_id: credential.id.as_uuid(),
        credential_generation: credential.generation(),
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

fn sample_binding() -> EdgeRouteBindingRef {
    EdgeRouteBindingRef::new(
        DomainClaimId::new(),
        GatewayScopeId::new(),
        "api.example.com",
        "/v1",
        1,
    )
    .unwrap()
}

async fn issue_credential(
    credentials: &InMemoryInferenceCredentialRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    prefix_suffix: &str,
) -> InferenceCredential {
    let now = Utc::now();
    let credential = InferenceCredential::issue(
        InferenceCredentialId::new(),
        organization_id,
        project_id,
        environment_id,
        format!("a3s_inf_{prefix_suffix}"),
        VERIFIER,
        now + Duration::hours(2),
        now,
    )
    .unwrap();
    credentials
        .create_inference_credential(credential.clone())
        .await
        .unwrap()
}

fn assert_grant_invalid(error: ApplicationError) {
    match error {
        ApplicationError::Invalid(message) => {
            assert!(
                message.starts_with(INFERENCE_GRANT_CREDENTIAL_INVALID),
                "expected {INFERENCE_GRANT_CREDENTIAL_INVALID} prefix, got {message}"
            );
        }
        other => panic!("expected ApplicationError::Invalid, got {other:?}"),
    }
}

fn publish_handler(
    credentials: Arc<InMemoryInferenceCredentialRepository>,
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> PublishInferenceRouteHandler {
    PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(IdentityInferenceGrantCredentialAdmissionAdapter::new(
            credentials,
        )),
    )
}

fn revise_handler(
    credentials: Arc<InMemoryInferenceCredentialRepository>,
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> ReviseInferenceRouteHandler {
    ReviseInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(IdentityInferenceGrantCredentialAdmissionAdapter::new(
            credentials,
        )),
    )
}

#[tokio::test]
async fn matching_active_credential_admits_and_publishes() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let credential = issue_credential(
        &credentials,
        organization_id,
        project_id,
        environment_id,
        "aaaaaaaaaaaaaaaa",
    )
    .await;
    let handler = publish_handler(credentials, routes);
    let route = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![grant_for(&credential)],
                binding: sample_binding(),
                idempotency_key: "grant-admit".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.grants()[0].credential_id, credential.id.as_uuid());
    assert_eq!(route.grants()[0].credential_generation, 1);
}

#[tokio::test]
async fn missing_credential_rejects_publish() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(credentials, routes);
    let error = handler
        .execute(
            PublishInferenceRoute {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![InferenceGrantAclProjection {
                    credential_id: Uuid::now_v7(),
                    credential_generation: 1,
                    models: vec!["chat-model".into()],
                    endpoints: vec![InferenceEndpointAcl::Models],
                    limits: InferenceLimitsAclProjection {
                        max_concurrent_requests: 1,
                        requests_per_minute: 10,
                        request_burst: 1,
                        tokens_per_minute: 100,
                    },
                }],
                binding: sample_binding(),
                idempotency_key: "grant-missing".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_grant_invalid(error);
}

#[tokio::test]
async fn wrong_environment_credential_rejects_publish() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment = EnvironmentId::new();
    let credential = issue_credential(
        &credentials,
        organization_id,
        project_id,
        other_environment,
        "bbbbbbbbbbbbbbbb",
    )
    .await;
    let handler = publish_handler(credentials, routes);
    let error = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![grant_for(&credential)],
                binding: sample_binding(),
                idempotency_key: "grant-wrong-env".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_grant_invalid(error);
}

#[tokio::test]
async fn revoked_credential_rejects_publish() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let mut credential = issue_credential(
        &credentials,
        organization_id,
        project_id,
        environment_id,
        "cccccccccccccccc",
    )
    .await;
    credential.revoke(Utc::now() + Duration::seconds(1)).unwrap();
    credentials
        .update_inference_credential(credential.clone(), 1)
        .await
        .unwrap();
    let handler = publish_handler(credentials, routes);
    let error = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![grant_for(&credential)],
                binding: sample_binding(),
                idempotency_key: "grant-revoked".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(2),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_grant_invalid(error);
}

#[tokio::test]
async fn stale_generation_rejects_publish() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let credential = issue_credential(
        &credentials,
        organization_id,
        project_id,
        environment_id,
        "dddddddddddddddd",
    )
    .await;
    let mut grant = grant_for(&credential);
    grant.credential_generation = 99;
    let handler = publish_handler(credentials, routes);
    let error = handler
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![grant],
                binding: sample_binding(),
                idempotency_key: "grant-stale-gen".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_grant_invalid(error);
}

#[tokio::test]
async fn revise_also_enforces_grant_credential_admission() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let first = issue_credential(
        &credentials,
        organization_id,
        project_id,
        environment_id,
        "eeeeeeeeeeeeeeee",
    )
    .await;
    let publish = publish_handler(Arc::clone(&credentials), Arc::clone(&routes));
    let published = publish
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![grant_for(&first)],
                binding: sample_binding(),
                idempotency_key: "publish-before-revise-grant".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let revise = revise_handler(credentials, routes);
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
                grants: vec![InferenceGrantAclProjection {
                    credential_id: Uuid::now_v7(),
                    credential_generation: 1,
                    models: vec!["chat-model".into()],
                    endpoints: vec![InferenceEndpointAcl::Models],
                    limits: InferenceLimitsAclProjection {
                        max_concurrent_requests: 1,
                        requests_per_minute: 10,
                        request_burst: 1,
                        tokens_per_minute: 100,
                    },
                }],
                binding: sample_binding(),
                idempotency_key: "revise-missing-grant".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert_grant_invalid(error);
}

#[tokio::test]
async fn port_admits_empty_grants() {
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let admission = IdentityInferenceGrantCredentialAdmissionAdapter::new(credentials);
    admission
        .admit(InferenceGrantCredentialAdmissionRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            Vec::new(),
            Utc::now(),
        ))
        .await
        .unwrap();
}
