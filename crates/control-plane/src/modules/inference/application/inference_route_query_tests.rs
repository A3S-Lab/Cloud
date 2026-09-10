//! First-principles authorized Inference route list/get queries (I0.2b brick).

use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::inference::application::{
    GetInferenceRoute, GetInferenceRouteHandler, IInferenceEnvironmentAccess,
    InferenceEnvironmentScope, ListInferenceRoutes, ListInferenceRoutesHandler,
    PermitInferenceEdgeRouteBindingAdmission, PermitInferenceGrantCredentialAdmission,
    PublishInferenceRoute, PublishInferenceRouteHandler, RetireInferenceRoute,
    RetireInferenceRouteHandler, DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::infrastructure::InMemoryInferenceRouteRepository;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    DomainEventEnvelope, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceTargetAclProjection,
};
use async_trait::async_trait;
use chrono::Utc;
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

#[async_trait]
impl IInferenceEnvironmentAccess for AlwaysPresentEnvironmentRepository {
    async fn environment_exists(
        &self,
        _scope: InferenceEnvironmentScope,
    ) -> Result<bool, RepositoryError> {
        Ok(true)
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn org_wide() -> ResourceAccessEvaluator {
    ResourceAccessEvaluator::organization_wide()
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

fn publish_handler(routes: Arc<InMemoryInferenceRouteRepository>) -> PublishInferenceRouteHandler {
    PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    )
}

async fn publish_one(
    handler: &PublishInferenceRouteHandler,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    key: &str,
) -> crate::modules::inference::domain::entities::InferenceRoute {
    handler
        .execute(
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
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn publish_then_list_and_get_return_route_without_secrets() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let list = ListInferenceRoutesHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let get =
        GetInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();

    let published = publish_one(
        &publish,
        organization_id,
        project_id,
        environment_id,
        "publish-list-get",
    )
    .await;

    let page = list
        .execute(
            ListInferenceRoutes {
                organization_id,
                project_id,
                environment_id,
                cursor: None,
                limit: DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(page.routes.len(), 1);
    assert_eq!(page.routes[0].id, published.id);
    assert!(page.next_cursor.is_none());
    assert!(page.routes[0].retired_at().is_none());

    let fetched = get
        .execute(
            GetInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, published.id);
    assert_eq!(fetched.policy_revision(), 1);
    assert_eq!(fetched.models().len(), 1);
    assert_eq!(fetched.grants().len(), 1);
}

#[tokio::test]
async fn retire_excludes_from_list_but_get_still_returns_retired_head() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let list = ListInferenceRoutesHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let get =
        GetInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();

    let published = publish_one(
        &publish,
        organization_id,
        project_id,
        environment_id,
        "publish-then-retire",
    )
    .await;
    retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                idempotency_key: "retire-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let page = list
        .execute(
            ListInferenceRoutes {
                organization_id,
                project_id,
                environment_id,
                cursor: None,
                limit: DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(page.routes.is_empty());

    let fetched = get
        .execute(
            GetInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.retired_at().is_some());
}

#[tokio::test]
async fn ungranted_environment_fails_closed_as_not_found() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let list = ListInferenceRoutesHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let get =
        GetInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish_one(
        &publish,
        organization_id,
        project_id,
        environment_id,
        "publish-denied-env",
    )
    .await;

    let denied = ResourceAccessEvaluator::restricted(vec![ResourceGrantScope::Environment {
        project_id,
        environment_id: EnvironmentId::from_uuid(Uuid::from_u128(999)),
    }]);

    let list_err = list
        .execute(
            ListInferenceRoutes {
                organization_id,
                project_id,
                environment_id,
                cursor: None,
                limit: DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
                resource_access: denied.clone(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(list_err, ApplicationError::NotFound(_)));

    let get_err = get
        .execute(
            GetInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                resource_access: denied,
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(get_err, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn get_rejects_wrong_environment_path_as_not_found() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let get = GetInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment_id = EnvironmentId::new();
    let published = publish_one(
        &publish,
        organization_id,
        project_id,
        environment_id,
        "publish-get-wrong-env",
    )
    .await;

    let denied = get
        .execute(
            GetInferenceRoute {
                organization_id,
                project_id,
                environment_id: other_environment_id,
                route_id: published.id,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn get_rejects_missing_environment_as_not_found() {
    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
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
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl IInferenceEnvironmentAccess for MissingEnvironmentRepository {
        async fn environment_exists(
            &self,
            _scope: InferenceEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let get = GetInferenceRouteHandler::new(Arc::new(MissingEnvironmentRepository), routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish_one(
        &publish,
        organization_id,
        project_id,
        environment_id,
        "publish-get-missing-env",
    )
    .await;

    let denied = get
        .execute(
            GetInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn list_rejects_missing_environment_as_not_found() {
    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
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
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl IInferenceEnvironmentAccess for MissingEnvironmentRepository {
        async fn environment_exists(
            &self,
            _scope: InferenceEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let list = ListInferenceRoutesHandler::new(Arc::new(MissingEnvironmentRepository), routes);
    let denied = list
        .execute(
            ListInferenceRoutes {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                cursor: None,
                limit: DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn list_pages_by_route_id_cursor() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let list = ListInferenceRoutesHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();

    let first = publish_one(&publish, organization_id, project_id, environment_id, "a").await;
    let second = publish_one(&publish, organization_id, project_id, environment_id, "b").await;
    let mut ordered = [first.id, second.id];
    ordered.sort();

    let page = list
        .execute(
            ListInferenceRoutes {
                organization_id,
                project_id,
                environment_id,
                cursor: None,
                limit: 1,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(page.routes.len(), 1);
    assert_eq!(page.routes[0].id, ordered[0]);
    let cursor = page.next_cursor.expect("next cursor");

    let next = list
        .execute(
            ListInferenceRoutes {
                organization_id,
                project_id,
                environment_id,
                cursor: Some(cursor),
                limit: 1,
                resource_access: org_wide(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.routes.len(), 1);
    assert_eq!(next.routes[0].id, ordered[1]);
    assert!(next.next_cursor.is_none());
}
