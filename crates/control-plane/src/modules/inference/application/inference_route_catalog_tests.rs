//! First-principles Inference route catalog authority tests (I0.2b brick).

use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope,
    PermitInferenceEdgeRouteBindingAdmission, PublishInferenceRoute, PublishInferenceRouteHandler,
    RetireInferenceRoute, RetireInferenceRouteHandler,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::infrastructure::{
    InferenceRouteAclProjectionAdapter, InMemoryInferenceRouteRepository,
};
use crate::modules::inference::EmptyInferenceRouteAclProjectionPort;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite,
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

fn publish_command(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    key: &str,
) -> PublishInferenceRoute {
    PublishInferenceRoute {
        organization_id,
        project_id,
        environment_id,
        router: "inference".into(),
        models: vec![sample_model()],
        grants: vec![sample_grant()],
        binding: sample_binding(),
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    }
}

fn publish_handler(
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> PublishInferenceRouteHandler {
    PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
    )
}

#[tokio::test]
async fn publish_lists_projection_with_models_and_grants_without_workers() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let route = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-1"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].route_id, route.id.as_uuid());
    assert_eq!(projections[0].models.len(), 1);
    assert_eq!(projections[0].grants.len(), 1);
    let rendered =
        a3s_cloud_contracts::render_inference_route_acl_blocks(&projections).unwrap();
    assert!(rendered.contains("models \"chat-model\""));
    assert!(rendered.contains("grants \"33333333-3333-4333-8333-333333333333\""));
    assert!(!rendered.contains("workers "));
}

#[tokio::test]
async fn empty_repository_projects_nothing_so_edge_invents_no_catalog_facts() {
    let adapter =
        InferenceRouteAclProjectionAdapter::new(Arc::new(InMemoryInferenceRouteRepository::default()));
    let scope = InferenceRouteEnvironmentScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
    )
    .unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert!(projections.is_empty());

    let empty = EmptyInferenceRouteAclProjectionPort;
    assert!(empty
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn retire_removes_route_from_projection() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let route = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-retire"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                route_id: route.id,
                idempotency_key: "retire-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    assert!(adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn publish_is_idempotent_for_same_key_and_body() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let binding = sample_binding();
    let command = PublishInferenceRoute {
        organization_id,
        project_id,
        environment_id,
        router: "inference".into(),
        models: vec![sample_model()],
        grants: vec![sample_grant()],
        binding,
        idempotency_key: "same-key".into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    };
    let first = handler
        .execute(command.clone(), context())
        .await
        .unwrap()
        .unwrap();
    let second = handler
        .execute(command, context())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.aggregate_version(), second.aggregate_version());
}

#[tokio::test]
async fn adapter_sorts_projections_stably_by_route_id() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let first = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "sort-a"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    let second = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "sort-b"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope, scope])
        .await
        .unwrap();
    assert_eq!(projections.len(), 2);
    assert!(projections[0].route_id < projections[1].route_id);
    let ids = [first.id.as_uuid(), second.id.as_uuid()];
    assert!(ids.contains(&projections[0].route_id));
    assert!(ids.contains(&projections[1].route_id));
}
