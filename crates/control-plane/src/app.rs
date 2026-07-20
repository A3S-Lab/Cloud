use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::{
    IDomainOwnershipVerifier, IGatewayCertificateAuthority, IGatewayCommandQueue,
    IRouteTargetReader,
};
use crate::modules::edge::{
    CreateDomainClaimHandler, DnsDomainOwnershipVerifier, EdgeDeploymentRouteUpdater,
    EdgeGatewayAcknowledgementProjector, EdgeModule, FleetGatewayCommandQueue,
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GetDomainClaimHandler, GetRouteHandler,
    ListDomainClaimsHandler, ListGatewayCertificatesHandler, ListRoutesHandler,
    LocalDomainOwnershipVerifier, LocalGatewayCertificateAuthority, PostgresEdgeRepository,
    PublishRouteHandler, VaultGatewayCertificateAuthority, VerifyDomainClaimHandler,
    WorkloadRouteTargetReader,
};
use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, INodeControlRepository, INodeRepository,
};
use crate::modules::fleet::domain::services::{ICertificateAuthority, ILogChunkStore};
use crate::modules::fleet::{
    AcknowledgeNodeCommandHandler, ChangeNodeStateHandler, EnqueueNodeCommandHandler,
    EnrollNodeHandler, FleetModule, GetNodeHandler, IGatewayAcknowledgementProjector,
    IssueEnrollmentTokenHandler, LeaseNodeCommandsHandler, ListNodesHandler,
    LocalCertificateAuthority, LocalKeyEncryptionService, LocalLogChunkStore, LogCompactionWorker,
    LogRetentionWorker, NodeControlApi, NodeControlServer, PostgresNodeRepository,
    RecordGatewayAcknowledgementHandler, RecordNodeLogChunksHandler, RecordNodeObservationsHandler,
    RotateNodeCertificateHandler, S3LogChunkStore, S3LogChunkStoreOptions,
    VaultCertificateAuthority, VaultKeyEncryptionService,
};
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
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::secrets::{
    CreateSecretHandler, GetSecretHandler, ListSecretsHandler, PostgresSecretRepository,
    RevokeSecretVersionHandler, RotateSecretHandler, SecretsModule,
};
use crate::modules::workloads::domain::repositories::ISecretRotationRestartRepository;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::domain::repositories::IWorkloadRuntimeTargetRepository;
use crate::modules::workloads::domain::services::{IDeploymentRouteUpdater, IOciArtifactResolver};
use crate::modules::workloads::{
    CancelDeploymentHandler, CreateWorkloadDeploymentHandler, DeploymentFlowConfig,
    DeploymentFlowRuntime, GetDeploymentHandler, GetWorkloadHandler, GetWorkloadLogsHandler,
    IWorkloadRuntimeControl, ListWorkloadsHandler, OciRegistryArtifactResolver,
    PostgresWorkloadRepository, RollbackWorkloadDeploymentHandler, SecretRotationRestartReconciler,
    StopWorkloadHandler, UpdateWorkloadDeploymentHandler, WorkloadRuntimeReconciler,
    WorkloadsModule,
};
use crate::modules::PlatformModule;
use crate::presentation::{ApiErrorFilter, ApiResponseInterceptor, RequestIdMiddleware};
use crate::server::{ControlPlane, ControlPlaneWorkers};
use crate::{
    config::{
        EventProviderKind, LogStorageProviderKind, ProcessRole, SecurityProfile,
        SecurityProviderKind,
    },
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
    #[error("could not initialize security providers: {0}")]
    Security(String),
    #[error("could not initialize Edge providers: {0}")]
    Edge(String),
    #[error("could not initialize log storage: {0}")]
    LogStorage(String),
    #[error("could not initialize node control: {0}")]
    NodeControl(String),
    #[error("could not initialize OCI registry access: {0}")]
    Registry(String),
    #[error("could not initialize Secret rotation restart reconciliation: {0}")]
    SecretRestart(String),
    #[error(transparent)]
    Framework(#[from] BootError),
}

pub async fn build_application(
    config: CloudConfig,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let postgres_url = config.postgres_url()?;
    let executor = connect_and_migrate(&postgres_url, config.postgres.max_connections).await?;
    let event_publisher = event_publisher(&config).await?;
    let vault_credentials = config.vault_credentials()?;
    let (certificate_authority, key_encryption) =
        security_providers(&config, vault_credentials.as_ref())?;
    let gateway_certificate_authority =
        gateway_certificate_authority(&config, vault_credentials.as_ref())?;
    let log_chunks = log_chunk_store(&config)?;
    let bootstrap_credential = BootstrapCredential::new(&config.bootstrap_token()?)
        .map_err(ControlPlaneStartupError::Auth)?;
    let identity = Arc::new(PostgresIdentityRepository::new(executor.clone()));
    let organizations: Arc<dyn IOrganizationRepository> = identity.clone();
    let api_tokens: Arc<dyn IApiTokenRepository> = identity;
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let nodes: Arc<dyn INodeRepository> = node_repository.clone();
    let node_control: Arc<dyn INodeControlRepository> = node_repository.clone();
    let log_retention_repository: Arc<dyn ILogRetentionRepository> = node_repository.clone();
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let workloads: Arc<dyn IWorkloadRepository> = workload_repository.clone();
    let workload_targets: Arc<dyn IWorkloadRuntimeTargetRepository> = workload_repository.clone();
    let secret_rotation_restarts: Arc<dyn ISecretRotationRestartRepository> = workload_repository;
    let workload_runtime_control: Arc<dyn IWorkloadRuntimeControl> = node_repository;
    let edge_repository = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let routes: Arc<dyn IEdgeRepository> = edge_repository;
    let secrets: Arc<dyn ISecretRepository> =
        Arc::new(PostgresSecretRepository::new(executor.clone()));
    let domain_verifier: Arc<dyn IDomainOwnershipVerifier> = match config.security.profile {
        SecurityProfile::Development => Arc::new(LocalDomainOwnershipVerifier),
        SecurityProfile::Production => Arc::new(
            DnsDomainOwnershipVerifier::from_system_config(Duration::from_millis(
                config.edge.domain_verification_timeout_ms,
            ))
            .map_err(|error| ControlPlaneStartupError::Edge(error.to_string()))?,
        ),
    };
    let gateway_projector: Arc<dyn IGatewayAcknowledgementProjector> = Arc::new(
        EdgeGatewayAcknowledgementProjector::new(Arc::clone(&routes)),
    );
    let route_targets: Arc<dyn IRouteTargetReader> = Arc::new(
        WorkloadRouteTargetReader::new(
            Arc::clone(&workloads),
            Arc::clone(&node_control),
            chrono_duration(config.fleet.heartbeat_timeout_ms)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
        )
        .map_err(ControlPlaneStartupError::NodeControl)?,
    );
    let route_commands: Arc<dyn IGatewayCommandQueue> =
        Arc::new(FleetGatewayCommandQueue::new(Arc::clone(&node_control)));
    let deployment_route_compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: config.edge.entrypoint_address.clone(),
        management_address: config.edge.management_address.clone(),
        management_path_prefix: config.edge.management_path_prefix.clone(),
        management_auth_token_env: config.edge.management_auth_token_env.clone(),
        upstream_request_timeout_ms: config.edge.upstream_request_timeout_ms,
        certificate_directory: config.edge.certificate_directory.clone(),
    })
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let deployment_route_updates: Arc<dyn IDeploymentRouteUpdater> = Arc::new(
        EdgeDeploymentRouteUpdater::new(
            Arc::clone(&routes),
            Arc::clone(&node_control),
            Arc::clone(&route_commands),
            deployment_route_compiler,
            chrono_duration(config.edge.command_ttl_ms)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
        )
        .map_err(ControlPlaneStartupError::NodeControl)?,
    );
    let artifacts: Arc<dyn IOciArtifactResolver> = Arc::new(
        OciRegistryArtifactResolver::new(
            Duration::from_millis(config.registry.request_timeout_ms),
            config.registry.insecure_hosts.clone(),
        )
        .map_err(ControlPlaneStartupError::Registry)?
        .with_registry_secret_material(Arc::clone(&secrets), Arc::clone(&key_encryption)),
    );
    let deployment_flow_config = DeploymentFlowConfig::from_milliseconds(
        config.deployments.command_ttl_ms,
        config.deployments.runtime_apply_timeout_ms,
        config.deployments.observation_poll_ms,
        config.deployments.convergence_timeout_ms,
        config.deployments.runtime_stop_timeout_ms,
        config.deployments.cleanup_poll_ms,
        config.deployments.cleanup_timeout_ms,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let deployment_runtime = DeploymentFlowRuntime::new(
        Arc::clone(&workloads),
        artifacts,
        Arc::clone(&nodes),
        Arc::clone(&node_control),
        deployment_route_updates,
        chrono_duration(config.fleet.heartbeat_timeout_ms)
            .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
        deployment_flow_config,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let flow =
        crate::infrastructure::connect_flow(&postgres_url, Arc::new(deployment_runtime)).await?;
    let run_node_control = matches!(config.server.role, ProcessRole::All | ProcessRole::Api);
    let node_control_server = if run_node_control {
        let api = NodeControlApi::new(
            Arc::clone(&nodes),
            Arc::clone(&node_control),
            Arc::clone(&gateway_projector),
            Arc::clone(&routes),
            Arc::clone(&gateway_certificate_authority),
            Arc::clone(&log_chunks),
            Arc::clone(&certificate_authority),
            Arc::clone(&workloads),
            Arc::clone(&secrets),
            Arc::clone(&key_encryption),
            chrono_duration(config.edge.certificate_ttl_ms)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
            chrono_duration(config.fleet.certificate_ttl_ms)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
            chrono_duration(config.fleet.certificate_rotation_window_ms)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
            chrono::Duration::try_milliseconds(
                i64::try_from(config.fleet.command_lease_ms).map_err(|_| {
                    ControlPlaneStartupError::NodeControl(
                        "command lease duration exceeds supported range".into(),
                    )
                })?,
            )
            .ok_or_else(|| {
                ControlPlaneStartupError::NodeControl(
                    "command lease duration exceeds supported range".into(),
                )
            })?,
            Duration::from_millis(config.fleet.command_long_poll_ms),
            Duration::from_millis(config.fleet.command_long_poll_ms.clamp(1, 50)),
            config.node_control.max_request_bytes,
            Duration::from_millis(config.node_control.request_body_timeout_ms),
        )
        .map_err(ControlPlaneStartupError::NodeControl)?;
        Some(
            NodeControlServer::from_config(&config.node_control, api)
                .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
        )
    } else {
        None
    };
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
    let operation_coordinator = crate::infrastructure::FlowOperationCoordinator::new(
        operation_reconciler,
        &flow,
        Duration::from_millis(config.operations.reconcile_interval_ms),
        Duration::from_millis(config.operations.lease_ms),
    )
    .map_err(|error| ControlPlaneStartupError::Framework(BootError::Internal(error.to_string())))?;
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
    let log_retention_worker = LogRetentionWorker::new(
        Arc::clone(&log_retention_repository),
        Arc::clone(&log_chunks),
        Duration::from_millis(config.logs.retention_ms),
        Duration::from_millis(config.logs.retention_poll_ms),
        config.logs.retention_batch_size,
    )
    .map_err(ControlPlaneStartupError::LogStorage)?;
    let log_compaction_worker = LogCompactionWorker::new(
        log_retention_repository,
        Duration::from_millis(config.logs.tombstone_retention_ms),
        Duration::from_millis(config.logs.tombstone_compaction_poll_ms),
        config.logs.tombstone_compaction_batch_size,
    )
    .map_err(ControlPlaneStartupError::LogStorage)?;
    let workload_reconciler = WorkloadRuntimeReconciler::new(
        workload_targets,
        workload_runtime_control,
        Duration::from_millis(config.deployments.reconcile_interval_ms),
        Duration::from_millis(config.deployments.command_ttl_ms),
        Duration::from_millis(config.deployments.runtime_apply_timeout_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let secret_rotation_restart_reconciler = SecretRotationRestartReconciler::new(
        secret_rotation_restarts,
        Duration::from_millis(config.deployments.reconcile_interval_ms),
        100,
        100,
    )
    .map_err(ControlPlaneStartupError::SecretRestart)?;
    let application = build_application_with_health(
        config,
        ApplicationDependencies {
            organizations,
            api_tokens,
            projects: projects.clone(),
            environments: projects,
            workloads,
            routes,
            secrets,
            secret_encryption: Arc::clone(&key_encryption),
            route_targets,
            route_commands,
            domain_verifier,
            gateway_projector,
            operations: operation_repository,
            nodes,
            node_control,
            log_chunks: log_chunks.clone(),
            certificate_authority: certificate_authority.clone(),
            bootstrap_credential,
            readiness: infrastructure_readiness(
                executor,
                flow,
                event_publisher,
                certificate_authority,
                gateway_certificate_authority,
                key_encryption,
                log_chunks,
            ),
        },
    )?;
    Ok(ControlPlane::new(
        application,
        ControlPlaneWorkers::new(
            run_operations.then_some(operation_coordinator),
            run_operations.then_some(secret_rotation_restart_reconciler),
            run_operations.then_some(workload_reconciler),
            run_operations.then_some(log_retention_worker),
            run_operations.then_some(log_compaction_worker),
            run_relay.then_some(outbox_relay),
            node_control_server,
        ),
    ))
}

struct ApplicationDependencies {
    organizations: Arc<dyn IOrganizationRepository>,
    api_tokens: Arc<dyn IApiTokenRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    routes: Arc<dyn IEdgeRepository>,
    secrets: Arc<dyn ISecretRepository>,
    secret_encryption: Arc<dyn ISecretEncryptionService>,
    route_targets: Arc<dyn IRouteTargetReader>,
    route_commands: Arc<dyn IGatewayCommandQueue>,
    domain_verifier: Arc<dyn IDomainOwnershipVerifier>,
    gateway_projector: Arc<dyn IGatewayAcknowledgementProjector>,
    operations: Arc<dyn IOperationRepository>,
    nodes: Arc<dyn INodeRepository>,
    node_control: Arc<dyn INodeControlRepository>,
    log_chunks: Arc<dyn ILogChunkStore>,
    certificate_authority: Arc<dyn ICertificateAuthority>,
    bootstrap_credential: BootstrapCredential,
    readiness: HealthModule,
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
        workloads,
        routes,
        secrets,
        secret_encryption,
        route_targets,
        route_commands,
        domain_verifier,
        gateway_projector,
        operations,
        nodes,
        node_control,
        log_chunks,
        certificate_authority,
        bootstrap_credential,
        readiness,
    } = dependencies;
    let project_organizations = Arc::clone(&organizations);
    let environment_projects = Arc::clone(&projects);
    let workload_environments = Arc::clone(&environments);
    let domain_environments = Arc::clone(&environments);
    let secret_environments = Arc::clone(&environments);
    let create_workloads = Arc::clone(&workloads);
    let workload_secrets = Arc::clone(&secrets);
    let update_workloads = Arc::clone(&workloads);
    let update_workload_secrets = Arc::clone(&secrets);
    let rollback_workloads = Arc::clone(&workloads);
    let rollback_workload_secrets = Arc::clone(&secrets);
    let cancel_workloads = Arc::clone(&workloads);
    let stop_workloads = Arc::clone(&workloads);
    let list_workloads = Arc::clone(&workloads);
    let get_workloads = Arc::clone(&workloads);
    let get_deployment_workloads = Arc::clone(&workloads);
    let get_log_workloads = Arc::clone(&workloads);
    let workload_list_operations = Arc::clone(&operations);
    let workload_get_operations = Arc::clone(&operations);
    let deployment_get_operations = Arc::clone(&operations);
    let query_organizations = Arc::clone(&organizations);
    let query_projects = Arc::clone(&projects);
    let query_environments = Arc::clone(&environments);
    let enrollment_nodes = Arc::clone(&nodes);
    let rotation_nodes = Arc::clone(&nodes);
    let state_nodes = Arc::clone(&nodes);
    let get_nodes = Arc::clone(&nodes);
    let enqueue_commands = Arc::clone(&node_control);
    let lease_commands = Arc::clone(&node_control);
    let acknowledge_commands = Arc::clone(&node_control);
    let observation_commands = Arc::clone(&node_control);
    let log_commands = Arc::clone(&node_control);
    let workload_list_observations = Arc::clone(&node_control);
    let workload_get_observations = Arc::clone(&node_control);
    let deployment_get_observations = Arc::clone(&node_control);
    let workload_log_metadata = Arc::clone(&node_control);
    let gateway_commands = node_control;
    let create_domain_claims = Arc::clone(&routes);
    let verify_domain_claims = Arc::clone(&routes);
    let publish_routes = Arc::clone(&routes);
    let list_domain_claims = Arc::clone(&routes);
    let get_domain_claims = Arc::clone(&routes);
    let list_gateway_certificates = Arc::clone(&routes);
    let list_routes = Arc::clone(&routes);
    let get_routes = routes;
    let create_secrets = Arc::clone(&secrets);
    let rotate_secrets = Arc::clone(&secrets);
    let revoke_secret_versions = Arc::clone(&secrets);
    let list_secrets = Arc::clone(&secrets);
    let get_secrets = secrets;
    let create_secret_encryption = Arc::clone(&secret_encryption);
    let rotate_secret_encryption = secret_encryption;
    let workload_log_store = Arc::clone(&log_chunks);
    let log_store = log_chunks;
    let heartbeat_timeout = chrono_duration(config.fleet.heartbeat_timeout_ms)?;
    let certificate_ttl = chrono_duration(config.fleet.certificate_ttl_ms)?;
    let command_lease = chrono_duration(config.fleet.command_lease_ms)?;
    let command_long_poll = Duration::from_millis(config.fleet.command_long_poll_ms);
    let command_poll_interval =
        Duration::from_millis(config.fleet.command_long_poll_ms.clamp(1, 50));
    let enroll_handler = EnrollNodeHandler::new(
        enrollment_nodes,
        Arc::clone(&certificate_authority),
        certificate_ttl,
        config.fleet.certificate_rotation_window_ms,
        config.fleet.heartbeat_interval_ms,
        config.fleet.command_long_poll_ms,
    )
    .map_err(BootError::Internal)?;
    let rotation_handler = RotateNodeCertificateHandler::new(
        rotation_nodes,
        Arc::clone(&certificate_authority),
        certificate_ttl,
    )
    .map_err(BootError::Internal)?;
    let route_compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: config.edge.entrypoint_address.clone(),
        management_address: config.edge.management_address.clone(),
        management_path_prefix: config.edge.management_path_prefix.clone(),
        management_auth_token_env: config.edge.management_auth_token_env.clone(),
        upstream_request_timeout_ms: config.edge.upstream_request_timeout_ms,
        certificate_directory: config.edge.certificate_directory.clone(),
    })
    .map_err(BootError::Internal)?;
    let publish_route_handler = PublishRouteHandler::new(
        publish_routes,
        route_targets,
        route_commands,
        route_compiler,
        chrono_duration(config.edge.command_ttl_ms)?,
    )
    .map_err(BootError::Internal)?;
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
                .command_handler::<crate::modules::secrets::CreateSecret, _>(
                    CreateSecretHandler::new(
                        secret_environments,
                        create_secrets,
                        create_secret_encryption,
                    ),
                )
                .command_handler::<crate::modules::secrets::RotateSecret, _>(
                    RotateSecretHandler::new(rotate_secrets, rotate_secret_encryption),
                )
                .command_handler::<crate::modules::secrets::RevokeSecretVersion, _>(
                    RevokeSecretVersionHandler::new(revoke_secret_versions),
                )
                .command_handler::<crate::modules::workloads::CreateWorkloadDeployment, _>(
                    CreateWorkloadDeploymentHandler::new(
                        workload_environments,
                        create_workloads,
                        workload_secrets,
                    ),
                )
                .command_handler::<crate::modules::workloads::UpdateWorkloadDeployment, _>(
                    UpdateWorkloadDeploymentHandler::new(update_workloads, update_workload_secrets),
                )
                .command_handler::<crate::modules::workloads::RollbackWorkloadDeployment, _>(
                    RollbackWorkloadDeploymentHandler::new(
                        rollback_workloads,
                        rollback_workload_secrets,
                    ),
                )
                .command_handler::<crate::modules::workloads::CancelDeployment, _>(
                    CancelDeploymentHandler::new(cancel_workloads),
                )
                .command_handler::<crate::modules::workloads::StopWorkload, _>(
                    StopWorkloadHandler::new(stop_workloads),
                )
                .command_handler::<crate::modules::edge::CreateDomainClaim, _>(
                    CreateDomainClaimHandler::new(domain_environments, create_domain_claims),
                )
                .command_handler::<crate::modules::edge::VerifyDomainClaim, _>(
                    VerifyDomainClaimHandler::new(verify_domain_claims, domain_verifier),
                )
                .command_handler::<crate::modules::edge::PublishRoute, _>(publish_route_handler)
                .command_handler::<crate::modules::fleet::IssueEnrollmentToken, _>(
                    IssueEnrollmentTokenHandler::new(
                        Arc::clone(&query_organizations),
                        Arc::clone(&nodes),
                    ),
                )
                .command_handler::<crate::modules::fleet::EnrollNode, _>(enroll_handler)
                .command_handler::<crate::modules::fleet::RotateNodeCertificate, _>(
                    rotation_handler,
                )
                .command_handler::<crate::modules::fleet::ChangeNodeState, _>(
                    ChangeNodeStateHandler::new(state_nodes, certificate_authority),
                )
                .command_handler::<crate::modules::fleet::EnqueueNodeCommand, _>(
                    EnqueueNodeCommandHandler::new(enqueue_commands),
                )
                .command_handler::<crate::modules::fleet::LeaseNodeCommands, _>(
                    LeaseNodeCommandsHandler::new(
                        lease_commands,
                        command_lease,
                        command_long_poll,
                        command_poll_interval,
                    )
                    .map_err(BootError::Internal)?,
                )
                .command_handler::<crate::modules::fleet::AcknowledgeNodeCommand, _>(
                    AcknowledgeNodeCommandHandler::new(acknowledge_commands),
                )
                .command_handler::<crate::modules::fleet::RecordNodeObservations, _>(
                    RecordNodeObservationsHandler::new(observation_commands),
                )
                .command_handler::<crate::modules::fleet::RecordNodeLogChunks, _>(
                    RecordNodeLogChunksHandler::new(log_commands, log_store),
                )
                .command_handler::<crate::modules::fleet::RecordGatewayAcknowledgement, _>(
                    RecordGatewayAcknowledgementHandler::new(gateway_commands, gateway_projector),
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
                .query_handler::<crate::modules::secrets::ListSecrets, _>(ListSecretsHandler::new(
                    list_secrets,
                ))
                .query_handler::<crate::modules::secrets::GetSecret, _>(GetSecretHandler::new(
                    get_secrets,
                ))
                .query_handler::<crate::modules::operations::ListOperations, _>(
                    ListOperationsHandler::new(operations),
                )
                .query_handler::<crate::modules::workloads::ListWorkloads, _>(
                    ListWorkloadsHandler::new(
                        list_workloads,
                        workload_list_operations,
                        workload_list_observations,
                    ),
                )
                .query_handler::<crate::modules::workloads::GetWorkload, _>(
                    GetWorkloadHandler::new(
                        get_workloads,
                        workload_get_operations,
                        workload_get_observations,
                    ),
                )
                .query_handler::<crate::modules::workloads::GetDeployment, _>(
                    GetDeploymentHandler::new(
                        get_deployment_workloads,
                        deployment_get_operations,
                        deployment_get_observations,
                    ),
                )
                .query_handler::<crate::modules::workloads::GetWorkloadLogs, _>(
                    GetWorkloadLogsHandler::new(
                        get_log_workloads,
                        workload_log_metadata,
                        workload_log_store,
                    ),
                )
                .query_handler::<crate::modules::fleet::GetNode, _>(
                    GetNodeHandler::new(get_nodes, heartbeat_timeout)
                        .map_err(BootError::Internal)?,
                )
                .query_handler::<crate::modules::fleet::ListNodes, _>(
                    ListNodesHandler::new(nodes, heartbeat_timeout).map_err(BootError::Internal)?,
                )
                .query_handler::<crate::modules::edge::ListRoutes, _>(ListRoutesHandler::new(
                    list_routes,
                ))
                .query_handler::<crate::modules::edge::ListDomainClaims, _>(
                    ListDomainClaimsHandler::new(list_domain_claims),
                )
                .query_handler::<crate::modules::edge::GetDomainClaim, _>(
                    GetDomainClaimHandler::new(get_domain_claims),
                )
                .query_handler::<crate::modules::edge::ListGatewayCertificates, _>(
                    ListGatewayCertificatesHandler::new(list_gateway_certificates),
                )
                .query_handler::<crate::modules::edge::GetRoute, _>(GetRouteHandler::new(
                    get_routes,
                ))
                .global(),
        )
        .import(IdentityModule::new(bootstrap_credential))
        .import(ProjectsModule)
        .import(SecretsModule)
        .import(OperationsModule)
        .import(FleetModule::new(heartbeat_timeout)?)
        .import(WorkloadsModule)
        .import(EdgeModule)
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
    certificate_authority: Arc<dyn ICertificateAuthority>,
    gateway_certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
    key_encryption: Arc<dyn ISecretEncryptionService>,
    log_chunks: Arc<dyn ILogChunkStore>,
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
        .indicator("certificate-authority", move || {
            let certificate_authority = certificate_authority.clone();
            async move {
                match certificate_authority.health().await {
                    Ok(true) => Ok(HealthIndicatorResult::up()),
                    Ok(false) => Ok(HealthIndicatorResult::down()),
                    Err(error) => {
                        Ok(HealthIndicatorResult::down()
                            .with_detail_value("error", error.to_string()))
                    }
                }
            }
        })
        .indicator("gateway-certificate-authority", move || {
            let gateway_certificate_authority = gateway_certificate_authority.clone();
            async move {
                match gateway_certificate_authority.health().await {
                    Ok(true) => Ok(HealthIndicatorResult::up()),
                    Ok(false) => Ok(HealthIndicatorResult::down()),
                    Err(error) => {
                        Ok(HealthIndicatorResult::down()
                            .with_detail_value("error", error.to_string()))
                    }
                }
            }
        })
        .indicator("key-encryption", move || {
            let key_encryption = key_encryption.clone();
            async move {
                match key_encryption.health().await {
                    Ok(true) => Ok(HealthIndicatorResult::up()),
                    Ok(false) => Ok(HealthIndicatorResult::down()),
                    Err(error) => {
                        Ok(HealthIndicatorResult::down()
                            .with_detail_value("error", error.to_string()))
                    }
                }
            }
        })
        .indicator("log-storage", move || {
            let log_chunks = log_chunks.clone();
            async move {
                match log_chunks.health().await {
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

type SecurityProviders = (
    Arc<dyn ICertificateAuthority>,
    Arc<dyn ISecretEncryptionService>,
);

fn security_providers(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<SecurityProviders, ControlPlaneStartupError> {
    let timeout = Duration::from_millis(config.security.vault_timeout_ms);
    let certificate_authority: Arc<dyn ICertificateAuthority> =
        match config.security.certificate_authority {
            SecurityProviderKind::Local => {
                let authority = LocalCertificateAuthority::load_or_create(
                    std::path::Path::new(&config.security.state_dir).join("node-ca"),
                )
                .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?;
                authority
                    .ensure_ca_bundle(std::path::Path::new(&config.node_control.client_ca_file))
                    .and_then(|()| {
                        authority.ensure_server_identity(
                            &config.node_control.server_name,
                            std::path::Path::new(&config.node_control.certificate_file),
                            std::path::Path::new(&config.node_control.private_key_file),
                        )
                    })
                    .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?;
                Arc::new(authority)
            }
            SecurityProviderKind::Vault => {
                let (address, token) = credentials.ok_or_else(|| {
                    ControlPlaneStartupError::Security("Vault credentials were not resolved".into())
                })?;
                Arc::new(
                    VaultCertificateAuthority::new(
                        address,
                        token,
                        config.security.vault_pki_mount.clone(),
                        config.security.vault_pki_role.clone(),
                        timeout,
                    )
                    .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
                )
            }
        };
    let key_encryption: Arc<dyn ISecretEncryptionService> = match config.security.key_encryption {
        SecurityProviderKind::Local => Arc::new(
            LocalKeyEncryptionService::load_or_create(
                std::path::Path::new(&config.security.state_dir).join("key-encryption.key"),
            )
            .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
        ),
        SecurityProviderKind::Vault => {
            let (address, token) = credentials.ok_or_else(|| {
                ControlPlaneStartupError::Security("Vault credentials were not resolved".into())
            })?;
            Arc::new(
                VaultKeyEncryptionService::new(
                    address,
                    token,
                    config.security.vault_transit_mount.clone(),
                    config.security.vault_transit_key.clone(),
                    timeout,
                )
                .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
            )
        }
    };
    Ok((certificate_authority, key_encryption))
}

fn gateway_certificate_authority(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn IGatewayCertificateAuthority>, ControlPlaneStartupError> {
    match config.security.gateway_certificate_authority {
        SecurityProviderKind::Local => Ok(Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(
                std::path::Path::new(&config.security.state_dir).join("gateway-ca"),
            )
            .map_err(|error| ControlPlaneStartupError::Edge(error.to_string()))?,
        )),
        SecurityProviderKind::Vault => {
            let (address, token) = credentials.ok_or_else(|| {
                ControlPlaneStartupError::Edge("Vault credentials were not resolved".into())
            })?;
            Ok(Arc::new(
                VaultGatewayCertificateAuthority::new(
                    address,
                    token,
                    config.security.vault_gateway_pki_mount.clone(),
                    config.security.vault_gateway_pki_role.clone(),
                    Duration::from_millis(config.security.vault_timeout_ms),
                )
                .map_err(|error| ControlPlaneStartupError::Edge(error.to_string()))?,
            ))
        }
    }
}

fn log_chunk_store(
    config: &CloudConfig,
) -> std::result::Result<Arc<dyn ILogChunkStore>, ControlPlaneStartupError> {
    match config.logs.storage_provider {
        LogStorageProviderKind::Local => Ok(Arc::new(
            LocalLogChunkStore::new(std::path::Path::new(&config.security.state_dir).join("logs"))
                .map_err(|error| ControlPlaneStartupError::LogStorage(error.to_string()))?,
        )),
        LogStorageProviderKind::S3 => {
            let credentials = config.s3_log_credentials()?.ok_or_else(|| {
                ControlPlaneStartupError::LogStorage("S3 credentials were not resolved".into())
            })?;
            Ok(Arc::new(
                S3LogChunkStore::new(S3LogChunkStoreOptions {
                    endpoint: (!config.logs.s3_endpoint.is_empty())
                        .then(|| config.logs.s3_endpoint.clone()),
                    region: config.logs.s3_region.clone(),
                    bucket: config.logs.s3_bucket.clone(),
                    prefix: config.logs.s3_prefix.clone(),
                    access_key_id: credentials.access_key_id,
                    secret_access_key: credentials.secret_access_key,
                    session_token: credentials.session_token,
                    allow_http: config.logs.s3_allow_http,
                    virtual_hosted_style: config.logs.s3_virtual_hosted_style,
                    request_timeout: Duration::from_millis(config.logs.s3_request_timeout_ms),
                    connect_timeout: Duration::from_millis(config.logs.s3_connect_timeout_ms),
                    retry_timeout: Duration::from_millis(config.logs.s3_retry_timeout_ms),
                    max_retries: config.logs.s3_max_retries,
                })
                .map_err(|error| ControlPlaneStartupError::LogStorage(error.to_string()))?,
            ))
        }
    }
}

fn chrono_duration(milliseconds: u64) -> Result<chrono::Duration> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| BootError::Internal("duration exceeds supported range".into()))
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
mod tests;
