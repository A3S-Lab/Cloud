use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::identity::domain::value_objects::BootstrapCredential;
use crate::modules::identity::infrastructure::ApiTokenVerifier;
use crate::modules::identity::{
    BootstrapIdentityHandler, CreateApiTokenHandler, CreateOrganizationHandler, IdentityModule,
    ListOrganizationsHandler, PostgresIdentityRepository, RevokeApiTokenHandler,
};
use crate::modules::integration_events::{
    A3sEventPublisher, EventPublishError, IEventPublisher, OutboxRelay, OutboxRelayConfig,
    PostgresOutboxRepository,
};
use crate::modules::operations::{
    FlowOperationEngine, IOperationRepository, ListOperationsHandler, OperationReconciler,
    OperationsModule, PostgresOperationRepository, ReconcileOperationsHandler,
};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::{
    CreateEnvironmentHandler, CreateProjectHandler, ListEnvironmentsHandler, ListProjectsHandler,
    PostgresProjectsRepository, ProjectsModule,
};
use crate::modules::PlatformModule;
use crate::presentation::{ApiErrorFilter, ApiResponseInterceptor, RequestIdMiddleware};
use crate::server::ControlPlane;
use crate::{
    config::{EventProviderKind, ProcessRole},
    infrastructure::{connect_and_migrate, postgres_health, PostgresBootstrapError},
    CloudConfig,
};
use a3s_boot::{
    AuthModule, BootApplication, BootError, CqrsModule, HealthIndicatorResult, HealthModule,
    Module, ModuleRef, OpenApiInfo, ProviderDefinition, ProviderToken, Result, RouteDefinition,
    AUTH_PUBLIC_METADATA,
};
use a3s_event::{NatsConfig, StorageType};
use a3s_orm::PostgresExecutor;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ControlPlaneStartupError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Postgres(#[from] PostgresBootstrapError),
    #[error(transparent)]
    Flow(#[from] crate::infrastructure::FlowInfrastructureError),
    #[error(transparent)]
    Event(#[from] EventPublishError),
    #[error("invalid authentication configuration: {0}")]
    Auth(String),
    #[error("invalid outbox relay configuration: {0}")]
    Outbox(String),
    #[error(transparent)]
    Framework(#[from] BootError),
}

pub async fn build_application(
    config: CloudConfig,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let postgres_url = config.postgres_url()?;
    let executor = connect_and_migrate(&postgres_url, config.postgres.max_connections).await?;
    let flow = crate::infrastructure::connect_flow(&postgres_url).await?;
    let event_publisher = event_publisher(&config).await?;
    let bootstrap_credential = BootstrapCredential::new(&config.bootstrap_token()?)
        .map_err(ControlPlaneStartupError::Auth)?;
    let identity = Arc::new(PostgresIdentityRepository::new(executor.clone()));
    let organizations: Arc<dyn IOrganizationRepository> = identity.clone();
    let api_tokens: Arc<dyn IApiTokenRepository> = identity;
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_engine = Arc::new(FlowOperationEngine::new(flow.engine()));
    let operation_reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            operation_engine,
        )),
        Duration::from_millis(config.operations.reconcile_interval_ms),
        100,
    );
    let outbox_relay = OutboxRelay::new(
        Arc::new(PostgresOutboxRepository::new(executor.clone())),
        event_publisher.clone(),
        OutboxRelayConfig {
            batch_size: config.events.batch_size,
            poll_interval: Duration::from_millis(config.events.poll_interval_ms),
            lease_duration: Duration::from_millis(config.events.lease_ms),
            publish_timeout: Duration::from_millis(config.events.publish_timeout_ms),
            initial_backoff: Duration::from_millis(config.events.retry_initial_ms),
            maximum_backoff: Duration::from_millis(config.events.retry_max_ms),
        },
    )
    .map_err(ControlPlaneStartupError::Outbox)?;
    let run_operations = matches!(config.server.role, ProcessRole::All | ProcessRole::Worker);
    let run_relay = matches!(config.server.role, ProcessRole::All | ProcessRole::Relay);
    let application = build_application_with_health(
        config,
        ApplicationDependencies {
            organizations,
            api_tokens,
            projects: projects.clone(),
            environments: projects,
            operations: operation_repository,
            bootstrap_credential,
            readiness: infrastructure_readiness(executor, flow, event_publisher),
        },
    )?;
    Ok(ControlPlane::new(
        application,
        run_operations.then_some(operation_reconciler),
        run_relay.then_some(outbox_relay),
    ))
}

struct ApplicationDependencies {
    organizations: Arc<dyn IOrganizationRepository>,
    api_tokens: Arc<dyn IApiTokenRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    operations: Arc<dyn IOperationRepository>,
    bootstrap_credential: BootstrapCredential,
    readiness: HealthModule,
}

#[cfg(test)]
pub(crate) fn build_application_with_repositories(
    config: CloudConfig,
    organizations: Arc<dyn IOrganizationRepository>,
    api_tokens: Arc<dyn IApiTokenRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    operations: Arc<dyn IOperationRepository>,
    bootstrap_credential: BootstrapCredential,
) -> Result<BootApplication> {
    build_application_with_health(
        config,
        ApplicationDependencies {
            organizations,
            api_tokens,
            projects,
            environments,
            operations,
            bootstrap_credential,
            readiness: HealthModule::new("readiness")
                .with_route("/health/ready")
                .indicator("repositories", || async { Ok(HealthIndicatorResult::up()) }),
        },
    )
}

fn build_application_with_health(
    config: CloudConfig,
    dependencies: ApplicationDependencies,
) -> Result<BootApplication> {
    let ApplicationDependencies {
        organizations,
        api_tokens,
        projects,
        environments,
        operations,
        bootstrap_credential,
        readiness,
    } = dependencies;
    let project_organizations = Arc::clone(&organizations);
    let environment_projects = Arc::clone(&projects);
    let query_organizations = Arc::clone(&organizations);
    let query_projects = Arc::clone(&projects);
    let query_environments = Arc::clone(&environments);
    BootApplication::builder()
        .import(PublicHealthModule::new(
            HealthModule::new("health")
                .with_route("/health/live")
                .indicator("process", || async { Ok(HealthIndicatorResult::up()) }),
        ))
        .import(PublicHealthModule::new(readiness))
        .import(
            AuthModule::new("cloud-auth")
                .bearer(ApiTokenVerifier::new(Arc::clone(&api_tokens)))
                .global(),
        )
        .import(
            CqrsModule::new("cloud-cqrs")
                .command_handler::<crate::modules::identity::BootstrapIdentity, _>(
                    BootstrapIdentityHandler::new(Arc::clone(&api_tokens)),
                )
                .command_handler::<crate::modules::identity::CreateApiToken, _>(
                    CreateApiTokenHandler::new(Arc::clone(&api_tokens)),
                )
                .command_handler::<crate::modules::identity::RevokeApiToken, _>(
                    RevokeApiTokenHandler::new(api_tokens),
                )
                .command_handler::<crate::modules::identity::CreateOrganization, _>(
                    CreateOrganizationHandler::new(organizations),
                )
                .command_handler::<crate::modules::projects::CreateProject, _>(
                    CreateProjectHandler::new(project_organizations, projects),
                )
                .command_handler::<crate::modules::projects::CreateEnvironment, _>(
                    CreateEnvironmentHandler::new(environment_projects, environments),
                )
                .query_handler::<crate::modules::identity::ListOrganizations, _>(
                    ListOrganizationsHandler::new(query_organizations),
                )
                .query_handler::<crate::modules::projects::ListProjects, _>(
                    ListProjectsHandler::new(query_projects),
                )
                .query_handler::<crate::modules::projects::ListEnvironments, _>(
                    ListEnvironmentsHandler::new(query_environments),
                )
                .query_handler::<crate::modules::operations::ListOperations, _>(
                    ListOperationsHandler::new(operations),
                )
                .global(),
        )
        .import(IdentityModule::new(bootstrap_credential))
        .import(ProjectsModule)
        .import(OperationsModule)
        .import(PlatformModule::new(&config))
        .use_global_middleware(RequestIdMiddleware)
        .use_global_auth()
        .use_global_interceptor(ApiResponseInterceptor)
        .use_global_filter(ApiErrorFilter)
        .global_prefix("/api/v1")
        .serve_openapi("/openapi.json", OpenApiInfo::new("A3S Cloud", "0.1.0"))
        .build()
}

#[derive(Clone)]
struct PublicHealthModule {
    inner: HealthModule,
}

impl PublicHealthModule {
    fn new(inner: HealthModule) -> Self {
        Self { inner }
    }
}

impl Module for PublicHealthModule {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        self.inner.providers()
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        self.inner.exports()
    }

    fn is_global(&self) -> bool {
        self.inner.is_global()
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        self.inner
            .routes()?
            .into_iter()
            .map(|route| route.with_metadata(AUTH_PUBLIC_METADATA, true))
            .collect()
    }

    fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        self.inner.on_module_init(module_ref)
    }
}

fn infrastructure_readiness(
    executor: PostgresExecutor,
    flow: crate::infrastructure::FlowInfrastructure,
    events: Arc<dyn IEventPublisher>,
) -> HealthModule {
    HealthModule::new("readiness")
        .with_route("/health/ready")
        .indicator("postgres", move || {
            let executor = executor.clone();
            async move { Ok(postgres_health(executor).await) }
        })
        .indicator("flow", move || {
            let flow = flow.clone();
            async move { Ok(flow.health().await) }
        })
        .indicator("events", move || {
            let events = events.clone();
            async move {
                match events.health().await {
                    Ok(true) => Ok(HealthIndicatorResult::up()),
                    Ok(false) => Ok(HealthIndicatorResult::down()),
                    Err(error) => {
                        Ok(HealthIndicatorResult::down()
                            .with_detail_value("error", error.to_string()))
                    }
                }
            }
        })
}

async fn event_publisher(
    config: &CloudConfig,
) -> std::result::Result<Arc<dyn IEventPublisher>, ControlPlaneStartupError> {
    match config.events.provider {
        EventProviderKind::Memory => Ok(Arc::new(A3sEventPublisher::memory())),
        EventProviderKind::Nats => {
            let url = config.nats_url()?.ok_or_else(|| {
                ControlPlaneStartupError::Outbox("NATS URL was not resolved".into())
            })?;
            let nats = NatsConfig {
                url,
                stream_name: config.events.stream_name.clone(),
                subject_prefix: "events".into(),
                storage: StorageType::File,
                ..NatsConfig::default()
            };
            Ok(Arc::new(A3sEventPublisher::nats(nats).await?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AuthConfig, EventProviderKind, EventsConfig, OperationsConfig, PostgresConfig, ProcessRole,
        ServerConfig,
    };
    use crate::modules::identity::domain::value_objects::ApiTokenScope;
    use crate::modules::identity::InMemoryIdentityRepository;
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::projects::InMemoryProjectsRepository;
    use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
    use serde_json::{json, Value};
    use uuid::Uuid;

    const BOOTSTRAP_TOKEN: &str = "test-bootstrap-credential-0123456789abcdef";
    const ADMIN_TOKEN: &str =
        "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PROJECT_TOKEN: &str =
        "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const EXPIRING_TOKEN: &str =
        "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn config() -> CloudConfig {
        CloudConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 8080,
                role: ProcessRole::All,
            },
            postgres: PostgresConfig {
                url_env: "A3S_CLOUD_POSTGRES_URL".into(),
                max_connections: 4,
            },
            auth: AuthConfig {
                bootstrap_token_env: "A3S_CLOUD_BOOTSTRAP_TOKEN".into(),
            },
            events: EventsConfig {
                provider: EventProviderKind::Memory,
                nats_url_env: "A3S_CLOUD_NATS_URL".into(),
                stream_name: "A3S_CLOUD_EVENTS".into(),
                batch_size: 100,
                poll_interval_ms: 250,
                lease_ms: 10_000,
                publish_timeout_ms: 3_000,
                retry_initial_ms: 500,
                retry_max_ms: 30_000,
            },
            operations: OperationsConfig {
                reconcile_interval_ms: 1_000,
                lease_ms: 5_000,
            },
        }
    }

    fn post_json(path: impl Into<String>, idempotency_key: &str, body: Value) -> BootRequest {
        post_json_as(path, idempotency_key, body, ADMIN_TOKEN)
    }

    fn post_json_as(
        path: impl Into<String>,
        idempotency_key: &str,
        body: Value,
        token: &str,
    ) -> BootRequest {
        BootRequest::new(HttpMethod::Post, path.into())
            .with_header("content-type", "application/json")
            .with_header("idempotency-key", idempotency_key)
            .with_header("authorization", format!("Bearer {token}"))
            .with_body(body.to_string().into_bytes())
    }

    fn delete_as(path: impl Into<String>, idempotency_key: &str, token: &str) -> BootRequest {
        BootRequest::new(HttpMethod::Delete, path.into())
            .with_header("idempotency-key", idempotency_key)
            .with_header("authorization", format!("Bearer {token}"))
    }

    fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
        BootRequest::new(HttpMethod::Get, path.into())
            .with_header("accept", "application/json")
            .with_header("authorization", format!("Bearer {token}"))
    }

    fn build_test_application(
        identity: Arc<InMemoryIdentityRepository>,
        projects: Arc<InMemoryProjectsRepository>,
    ) -> Result<BootApplication> {
        build_application_with_repositories(
            config(),
            identity.clone(),
            identity,
            projects.clone(),
            projects,
            Arc::new(InMemoryOperationRepository::new()),
            BootstrapCredential::new(BOOTSTRAP_TOKEN).map_err(BootError::Internal)?,
        )
    }

    fn response_json(response: &BootResponse) -> Result<Value> {
        response.body_json()
    }

    fn response_id(response: &BootResponse) -> Result<String> {
        response_json(response)?["data"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| BootError::Internal("response does not contain a resource ID".into()))
    }

    async fn create_organization(
        app: &BootApplication,
        idempotency_key: &str,
        name: &str,
    ) -> Result<String> {
        let response = app
            .call(post_json(
                "/api/v1/organizations",
                idempotency_key,
                json!({"name": name}),
            ))
            .await?;
        assert_eq!(response.status(), 201);
        response_id(&response)
    }

    async fn bootstrap_organization(
        app: &BootApplication,
        idempotency_key: &str,
        name: &str,
    ) -> Result<String> {
        let response = app
            .call(
                post_json(
                    "/api/v1/bootstrap",
                    idempotency_key,
                    json!({
                        "organizationName": name,
                        "tokenName": "bootstrap-admin",
                        "token": ADMIN_TOKEN,
                        "expiresAt": null
                    }),
                )
                .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN),
            )
            .await?;
        assert_eq!(response.status(), 201);
        response_json(&response)?["data"]["organization"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| BootError::Internal("bootstrap response has no organization ID".into()))
    }

    async fn create_project(
        app: &BootApplication,
        organization_id: &str,
        idempotency_key: &str,
        name: &str,
    ) -> Result<String> {
        let response = app
            .call(post_json(
                format!("/api/v1/organizations/{organization_id}/projects"),
                idempotency_key,
                json!({"name": name}),
            ))
            .await?;
        assert_eq!(response.status(), 201);
        response_id(&response)
    }

    async fn create_api_token(
        app: &BootApplication,
        organization_id: &str,
        idempotency_key: &str,
        name: &str,
        secret: &str,
        scopes: &[&str],
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<String> {
        let response = app
            .call(post_json(
                format!("/api/v1/organizations/{organization_id}/api-tokens"),
                idempotency_key,
                json!({
                    "name": name,
                    "token": secret,
                    "scopes": scopes,
                    "expiresAt": expires_at,
                }),
            ))
            .await?;
        assert_eq!(response.status(), 201);
        assert!(!String::from_utf8_lossy(response.body()).contains(secret));
        response_id(&response)
    }

    #[tokio::test]
    async fn boot_shell_exposes_wrapped_platform_and_health_responses() -> Result<()> {
        let organizations = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(organizations, projects)?;
        let platform = app
            .call(
                BootRequest::new(HttpMethod::Get, "/api/v1/platform")
                    .with_header("accept", "application/json")
                    .with_header("x-request-id", "018f3f56-8d4a-7c2a-9f13-5ab3d245d701"),
            )
            .await?;
        let body = response_json(&platform)?;
        assert_eq!(platform.status(), 200);
        assert_eq!(body["code"], 200);
        assert_eq!(body["data"]["name"], "a3s-cloud");
        assert_eq!(body["requestId"], "018f3f56-8d4a-7c2a-9f13-5ab3d245d701");

        let health = app
            .call(
                BootRequest::new(HttpMethod::Get, "/api/v1/health/live")
                    .with_header("accept", "application/json"),
            )
            .await?;
        let body = response_json(&health)?;
        assert_eq!(body["data"]["status"], "up");

        let readiness = app
            .call(
                BootRequest::new(HttpMethod::Get, "/api/v1/health/ready")
                    .with_header("accept", "application/json"),
            )
            .await?;
        let body = response_json(&readiness)?;
        assert_eq!(body["data"]["status"], "up");
        Ok(())
    }

    #[tokio::test]
    async fn organization_writes_are_idempotent_unique_and_atomic() -> Result<()> {
        let repository = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(repository.clone(), projects)?;
        bootstrap_organization(&app, "bootstrap-root", "Root").await?;
        let request = || {
            post_json(
                "/api/v1/organizations",
                "create-acme",
                json!({"name": "Acme"}),
            )
        };

        let first = app.call(request()).await?;
        let second = app.call(request()).await?;
        let first_body = response_json(&first)?;
        let second_body = response_json(&second)?;
        assert_eq!(first.status(), 201);
        assert_eq!(second.status(), 200);
        assert_eq!(first_body["data"]["id"], second_body["data"]["id"]);
        assert_eq!(second_body["data"]["replayed"], true);

        let changed = app
            .call(post_json(
                "/api/v1/organizations",
                "create-acme",
                json!({"name": "Other"}),
            ))
            .await?;
        assert_eq!(changed.status(), 409);
        assert_eq!(response_json(&changed)?["statusCode"], "CONFLICT");

        let duplicate = app
            .call(post_json(
                "/api/v1/organizations",
                "duplicate-acme",
                json!({"name": "acme"}),
            ))
            .await?;
        assert_eq!(duplicate.status(), 409);
        assert_eq!(repository.outbox_events().await.len(), 3);
        Ok(())
    }

    #[tokio::test]
    async fn project_writes_are_idempotent_and_names_are_organization_scoped() -> Result<()> {
        let organizations = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(organizations, projects.clone())?;
        let acme = bootstrap_organization(&app, "organization-acme", "Acme").await?;
        let beta = create_organization(&app, "organization-beta", "Beta").await?;
        let path = format!("/api/v1/organizations/{acme}/projects");
        let request = || post_json(&path, "project-cloud", json!({"name": "Cloud"}));

        let first = app.call(request()).await?;
        let second = app.call(request()).await?;
        assert_eq!(first.status(), 201);
        assert_eq!(second.status(), 200);
        assert_eq!(response_id(&first)?, response_id(&second)?);
        assert_eq!(response_json(&second)?["data"]["replayed"], true);

        let changed = app
            .call(post_json(&path, "project-cloud", json!({"name": "Other"})))
            .await?;
        assert_eq!(changed.status(), 409);

        let duplicate = app
            .call(post_json(
                &path,
                "project-cloud-duplicate",
                json!({"name": "cloud"}),
            ))
            .await?;
        assert_eq!(duplicate.status(), 409);

        let other_scope = app
            .call(post_json(
                format!("/api/v1/organizations/{beta}/projects"),
                "project-cloud",
                json!({"name": "Cloud"}),
            ))
            .await?;
        assert_eq!(other_scope.status(), 201);
        let events = projects.outbox_events().await;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_key == "project.project.created")
                .count(),
            2
        );
        Ok(())
    }

    #[tokio::test]
    async fn environment_writes_are_idempotent_and_names_are_project_scoped() -> Result<()> {
        let organizations = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(organizations, projects.clone())?;
        let organization = bootstrap_organization(&app, "organization", "Acme").await?;
        let cloud = create_project(&app, &organization, "project-cloud", "Cloud").await?;
        let data = create_project(&app, &organization, "project-data", "Data").await?;
        let path = format!("/api/v1/organizations/{organization}/projects/{cloud}/environments");
        let request = || {
            post_json(
                &path,
                "environment-production",
                json!({"name": "Production"}),
            )
        };

        let first = app.call(request()).await?;
        let second = app.call(request()).await?;
        assert_eq!(first.status(), 201);
        assert_eq!(second.status(), 200);
        assert_eq!(response_id(&first)?, response_id(&second)?);
        assert_eq!(response_json(&second)?["data"]["replayed"], true);

        let changed = app
            .call(post_json(
                &path,
                "environment-production",
                json!({"name": "Staging"}),
            ))
            .await?;
        assert_eq!(changed.status(), 409);

        let duplicate = app
            .call(post_json(
                &path,
                "environment-production-duplicate",
                json!({"name": "production"}),
            ))
            .await?;
        assert_eq!(duplicate.status(), 409);

        let other_scope = app
            .call(post_json(
                format!("/api/v1/organizations/{organization}/projects/{data}/environments"),
                "environment-production",
                json!({"name": "Production"}),
            ))
            .await?;
        assert_eq!(other_scope.status(), 201);
        let events = projects.outbox_events().await;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_key == "project.environment.created")
                .count(),
            2
        );
        Ok(())
    }

    #[tokio::test]
    async fn projects_and_environments_reject_cross_tenant_references() -> Result<()> {
        let organizations = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(organizations, projects.clone())?;
        let organization_id = bootstrap_organization(&app, "organization", "Acme").await?;
        let project_id = create_project(&app, &organization_id, "project", "Cloud").await?;

        let wrong_organization = Uuid::new_v4();
        let rejected = app
            .call(post_json(
                format!(
                    "/api/v1/organizations/{wrong_organization}/projects/{project_id}/environments"
                ),
                "wrong-environment",
                json!({"name": "Production"}),
            ))
            .await?;
        let rejected_body = response_json(&rejected)?;
        assert_eq!(rejected.status(), 404);
        assert_eq!(rejected_body["statusCode"], "NOT_FOUND");

        let environment = app
            .call(post_json(
                format!(
                    "/api/v1/organizations/{organization_id}/projects/{project_id}/environments"
                ),
                "environment",
                json!({"name": "Production"}),
            ))
            .await?;
        assert_eq!(environment.status(), 201);
        assert_eq!(projects.outbox_events().await.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn bearer_tokens_are_scoped_to_one_organization_and_never_echoed() -> Result<()> {
        let identity = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(identity, projects)?;
        let acme = bootstrap_organization(&app, "bootstrap-acme", "Acme").await?;
        let beta = create_organization(&app, "organization-beta", "Beta").await?;
        create_api_token(
            &app,
            &acme,
            "token-projects",
            "project-automation",
            PROJECT_TOKEN,
            &[ApiTokenScope::PROJECT_WRITE],
            None,
        )
        .await?;

        let no_credentials = app
            .call(
                BootRequest::new(
                    HttpMethod::Post,
                    format!("/api/v1/organizations/{acme}/projects"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "unauthenticated")
                .with_body(json!({"name": "Rejected"}).to_string().into_bytes()),
            )
            .await?;
        assert_eq!(no_credentials.status(), 401);

        let own_project = app
            .call(post_json_as(
                format!("/api/v1/organizations/{acme}/projects"),
                "project-own",
                json!({"name": "Own"}),
                PROJECT_TOKEN,
            ))
            .await?;
        assert_eq!(own_project.status(), 201);

        let cross_tenant = app
            .call(post_json_as(
                format!("/api/v1/organizations/{beta}/projects"),
                "project-cross-tenant",
                json!({"name": "Rejected"}),
                PROJECT_TOKEN,
            ))
            .await?;
        assert_eq!(cross_tenant.status(), 403);

        let scope_escalation = app
            .call(post_json_as(
                format!("/api/v1/organizations/{acme}/api-tokens"),
                "scope-escalation",
                json!({
                    "name": "Escalated",
                    "token": EXPIRING_TOKEN,
                    "scopes": [ApiTokenScope::TOKEN_WRITE],
                    "expiresAt": null
                }),
                PROJECT_TOKEN,
            ))
            .await?;
        assert_eq!(scope_escalation.status(), 403);
        Ok(())
    }

    #[tokio::test]
    async fn revoked_and_expired_tokens_stop_authenticating_immediately() -> Result<()> {
        let identity = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(identity, projects)?;
        let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
        let project_token_id = create_api_token(
            &app,
            &organization,
            "token-revoked",
            "revoked-token",
            PROJECT_TOKEN,
            &[ApiTokenScope::PROJECT_WRITE],
            None,
        )
        .await?;
        let revoke_path =
            format!("/api/v1/organizations/{organization}/api-tokens/{project_token_id}");
        let revoked = app
            .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
            .await?;
        assert_eq!(revoked.status(), 200);
        assert!(response_json(&revoked)?["data"]["revokedAt"].is_string());
        let replayed = app
            .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
            .await?;
        assert_eq!(response_json(&replayed)?["data"]["replayed"], true);

        let revoked_use = app
            .call(post_json_as(
                format!("/api/v1/organizations/{organization}/projects"),
                "revoked-use",
                json!({"name": "Rejected"}),
                PROJECT_TOKEN,
            ))
            .await?;
        assert_eq!(revoked_use.status(), 401);

        create_api_token(
            &app,
            &organization,
            "token-expiring",
            "expiring-token",
            EXPIRING_TOKEN,
            &[ApiTokenScope::PROJECT_WRITE],
            Some(chrono::Utc::now() + chrono::Duration::milliseconds(40)),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(60)).await;
        let expired_use = app
            .call(post_json_as(
                format!("/api/v1/organizations/{organization}/projects"),
                "expired-use",
                json!({"name": "Rejected"}),
                EXPIRING_TOKEN,
            ))
            .await?;
        assert_eq!(expired_use.status(), 401);
        Ok(())
    }

    #[tokio::test]
    async fn authenticated_queries_and_operation_stream_return_authoritative_snapshots(
    ) -> Result<()> {
        let identity = Arc::new(InMemoryIdentityRepository::new());
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let app = build_test_application(identity, projects)?;
        let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
        let project = create_project(&app, &organization, "project", "Cloud").await?;
        let environment_path =
            format!("/api/v1/organizations/{organization}/projects/{project}/environments");
        let environment = app
            .call(post_json(
                &environment_path,
                "environment",
                json!({"name": "Production"}),
            ))
            .await?;
        assert_eq!(environment.status(), 201);

        let organizations = app
            .call(get_as("/api/v1/organizations", ADMIN_TOKEN))
            .await?;
        assert_eq!(response_json(&organizations)?["data"][0]["name"], "Acme");
        let listed_projects = app
            .call(get_as(
                format!("/api/v1/organizations/{organization}/projects"),
                ADMIN_TOKEN,
            ))
            .await?;
        assert_eq!(response_json(&listed_projects)?["data"][0]["name"], "Cloud");
        let environments = app.call(get_as(&environment_path, ADMIN_TOKEN)).await?;
        assert_eq!(
            response_json(&environments)?["data"][0]["name"],
            "Production"
        );
        let operations = app
            .call(get_as(
                format!("/api/v1/organizations/{organization}/operations"),
                ADMIN_TOKEN,
            ))
            .await?;
        assert_eq!(response_json(&operations)?["data"], json!([]));

        let stream = app
            .call(
                BootRequest::new(
                    HttpMethod::Get,
                    format!("/api/v1/organizations/{organization}/operations/stream"),
                )
                .with_header("accept", "text/event-stream")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
            )
            .await?;
        assert_eq!(stream.status(), 200);
        assert!(stream.is_streaming());
        assert!(stream.is_event_stream());
        Ok(())
    }
}
