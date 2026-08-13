use crate::infrastructure::{
    ImmutableObjectClient, OperationResourceAccessResolver, S3ImmutableObjectOptions,
};
use crate::modules::agents::{
    AgentExecutionFlowRuntime, AgentExecutionFlowRuntimeDependencies, AgentExecutionReconciler,
    AgentsModule, AppendAgentExecutionEventsHandler, CancelAgentExecutionHandler,
    CreateAgentConversationHandler, GetAgentConversationHandler, GetAgentExecutionChangeSetHandler,
    GetAgentExecutionEventsHandler, GetAgentExecutionHandler, IAgentRepository,
    ListAgentConversationsHandler, ListAgentExecutionsHandler, PostgresAgentRepository,
    StartAgentExecutionHandler,
};
use crate::modules::artifacts::application::BuildRunReconciler;
use crate::modules::artifacts::{
    ArtifactsModule, BoxBuildEvidenceGenerator, BuildFlowRuntime, BuildFlowRuntimeDependencies,
    CancelBuildRunHandler, CloudBuildSourceResolver, GetBuildEvidenceHandler, GetBuildRunHandler,
    GetBuildRunLogsHandler, IBuildArtifactPublisher, IBuildEvidenceGenerator, IBuildEvidenceSigner,
    IBuildInputPreparer, IBuildOutputValidator, IBuildRunRepository, IBuildSourceResolver,
    INodeArtifactStore, ListBuildRunsHandler, LocalBuildEvidenceSigner, LocalNodeArtifactStore,
    OciBuildOutputValidator, OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions,
    PostgresBuildRunRepository, RetryBuildRunHandler, SourceBuildInputPreparer,
    VaultBuildEvidenceSigner,
};
use crate::modules::assets::{
    AdmitAssetManifestHandler, AdvertiseAssetGitRepositoryHandler, ArchiveAssetHandler,
    AssetCatalogApplicationService, AssetGitApplicationService, AssetGitApplicationServiceOptions,
    AssetsModule, BackupAssetGitRepositoryHandler, BindMcpServiceProfileHandler,
    CreateAssetHandler, CreateAssetReleaseHandler, GetAssetHandler, GetAssetReleaseHandler,
    GetMcpServiceProfileHandler, IAssetGitRepository, IAssetGitRepositoryControl, IAssetRepository,
    IMcpServiceProfileRepository, ListAssetReleasesHandler, ListAssetsHandler,
    LocalAssetGitRepository, McpServiceProfileApplicationService, PostgresAssetRepository,
    ReceiveAssetGitPackHandler, RestoreAssetGitRepositoryHandler, SelectAssetReleaseHandler,
    UploadAssetGitPackHandler, YankAssetReleaseHandler,
};
use crate::modules::edge::domain::repositories::{
    IEdgeRepository, IMcpCredentialLifecycleRepository, IMcpRoutePolicyRepository,
};
use crate::modules::edge::domain::services::{
    IDomainOwnershipVerifier, IGatewayCertificateAuthority, IGatewayCommandQueue,
    IGatewayObservationQueue, IMcpCredentialIssuer, IRouteTargetReader,
};
use crate::modules::edge::{
    CreateDomainClaimHandler, CreateGatewayScopeHandler, CreateMcpCredentialHandler,
    CreateMcpRoutePolicyHandler, DnsDomainOwnershipVerifier, EdgeDeploymentRouteUpdater,
    EdgeGatewayAcknowledgementProjector, EdgeModule, FleetGatewayCommandQueue,
    FleetGatewayObservationQueue, GatewayCertificateReconciler, GatewayNodeDesiredStatePlanner,
    GatewayReplicaRecoveryReconciler, GatewayRolloutReconciler, GatewayRolloutRollbackCompiler,
    GatewayRolloutRollbackReconciler, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GetDomainClaimHandler, GetMcpCredentialHandler, GetMcpRoutePolicyHandler, GetRouteHandler,
    ListDomainClaimsHandler, ListGatewayCertificatesHandler, ListGatewayScopesHandler,
    ListMcpCredentialsHandler, ListMcpRoutePoliciesHandler, ListRoutesHandler,
    LocalDomainOwnershipVerifier, LocalGatewayCertificateAuthority,
    McpCredentialDeliveryReceiptSweeper, McpCredentialIssuer, McpGatewayDesiredStateReconciler,
    McpGatewayNodeProjectionPlanner, McpGatewayProjectionAssembler, McpGatewayProjectionPlanner,
    McpGatewayProjectionSetPlanner, McpGatewaySnapshotReconciler, McpRoutePolicyApplicationService,
    McpRouteProjectionInputReader, McpRouteProjectionPlanner, McpRouteTargetProjectionCompiler,
    PostgresEdgeRepository, PublishRouteHandler, ReviseMcpRoutePolicyHandler,
    RevokeDomainClaimHandler, RevokeMcpCredentialHandler, RotateMcpCredentialHandler,
    VaultGatewayCertificateAuthority, VerifyDomainClaimHandler, WorkloadRouteTargetReader,
};
use crate::modules::executions::{
    CancelExecutionHandler, CreateExecutionHandler, CreateExecutionTemplateHandler,
    ExecutionFlowRuntime, ExecutionFlowRuntimeDependencies, ExecutionReconciler, ExecutionsModule,
    GetExecutionHandler, GetExecutionTemplateHandler, IExecutionRepository,
    IExecutionTemplateRepository, IWorkflowExecutionPort, ListExecutionTemplatesHandler,
    ListExecutionsHandler, PostgresExecutionRepository, PostgresExecutionTemplateRepository,
    WorkflowExecutionApplicationService,
};
use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, INodeControlRepository, INodeDrainRepository, INodePoolRepository,
    INodeRepository, INodeSchedulingRepository,
};
use crate::modules::fleet::domain::services::{ICertificateAuthority, ILogChunkStore};
use crate::modules::fleet::{
    AcknowledgeNodeCommandHandler, ChangeNodeStateHandler, EnqueueNodeCommandHandler,
    EnrollNodeHandler, FleetModule, GetNodeHandler, GetNodePoolHandler,
    IGatewayAcknowledgementProjector, IssueEnrollmentTokenHandler, LeaseNodeCommandsHandler,
    ListNodePoolsHandler, ListNodesHandler, LocalCertificateAuthority, LocalKeyEncryptionService,
    LogChunkObjectStore, LogCompactionWorker, LogRetentionWorker, ManageNodePoolHandler,
    NodeControlApi, NodeControlServer, PostgresNodeRepository, RecordGatewayAcknowledgementHandler,
    RecordNodeLogChunksHandler, RecordNodeObservationsHandler, RotateNodeCertificateHandler,
    VaultCertificateAuthority, VaultKeyEncryptionService,
};
use crate::modules::forms::{
    CreateFormDraftHandler, FormsModule, GetFormDraftHandler, GetFormReleaseHandler,
    IFormRepository, IFormSemanticCore, ListFormDraftsHandler, ListFormReleasesHandler,
    NativeFormSemanticCore, PostgresFormRepository, PublishFormReleaseHandler,
    ReviseFormDraftHandler,
};
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IMembershipRepository, IOrganizationRepository,
    IResourceAuthorizationDecisionRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::BootstrapCredential;
use crate::modules::identity::infrastructure::ApiTokenVerifier;
use crate::modules::identity::{
    BootstrapIdentityHandler, ChangeMembershipRoleHandler, CreateApiTokenHandler,
    CreateOrganizationHandler, CreateResourceGrantHandler, CreateServiceMembershipHandler,
    GetApiTokenHandler, GetMembershipHandler, GetResourceGrantHandler, IdentityModule,
    ListApiTokensHandler, ListMembershipsHandler, ListOrganizationsHandler,
    ListResourceGrantsHandler, PostgresIdentityRepository, RevokeApiTokenHandler,
    RevokeMembershipHandler, RevokeResourceGrantHandler,
};
use crate::modules::integration_events::{
    A3sEventPublisher, EventPublishError, IEventPublisher, OutboxRelay, OutboxRelayConfig,
    PostgresOutboxRepository,
};
use crate::modules::operations::{
    FlowOperationEngine, IOperationRepository, ListOperationsHandler, OperationReconciler,
    OperationsModule, PostgresOperationRepository, ReconcileOperationsHandler,
};
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginRegistryEnrollmentAuthorizer, IPluginTrustRootStore,
};
use crate::modules::plugins::{
    A3sUsePluginRegistryCatalog, EnrollPluginRegistryHandler, GetPluginRegistryHandler,
    InspectCachedPluginCatalogHandler, InspectPluginCatalogHandler, ListPluginRegistriesHandler,
    PluginTrustRootObjectStore, PluginsModule, PostgresPluginRegistryRepository,
    SearchCachedPluginCatalogHandler, SearchPluginCatalogHandler,
};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::{
    CreateEnvironmentHandler, CreateProjectHandler, ListEnvironmentsHandler, ListProjectsHandler,
    PostgresProjectsRepository, ProjectsModule,
};
use crate::modules::search::{
    ISearchRepository, PostgresSearchRepository, SearchModule, SearchResourcesHandler,
};
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::secrets::{
    CreateSecretHandler, GetSecretHandler, ListSecretsHandler, PostgresSecretRepository,
    RevokeSecretVersionHandler, RotateSecretHandler, SecretsModule,
};
use crate::modules::sources::domain::{
    IGithubAppAuthorizationService, IGithubConnectionAuthorityService, IGithubConnectionRepository,
    IGithubInstallationAuthorityProvider, IGithubInstallationTokenService, ISourceCheckout,
    ISourceResolver, ISourceRevisionRepository, ISourceSubscriptionRepository,
    ISourceWebhookRepository, ISourceWebhookVerifier, SourceRepositoryPolicy,
};
use crate::modules::sources::{
    AcceptSourceWebhookDeliveryHandler, BeginGithubConnectionHandler,
    CompleteGithubConnectionHandler, CreateGithubRepositorySubscriptionHandler,
    DeactivateGithubRepositorySubscriptionHandler, GetGithubConnectionHandler, GitSourceCheckout,
    GithubAppClient, GithubConnectionAuthorityReconciler, GithubInstallationTokenIssuer,
    GithubSourceResolver, GithubWebhookVerifier, ListGithubRepositorySubscriptionsHandler,
    ListSourceRevisionsHandler, PostgresGithubConnectionRepository,
    PostgresSourceRevisionRepository, PostgresSourceSubscriptionRepository,
    PrepareGithubConnectionOauthHandler, ReconcileGithubConnectionLifecycleHandler,
    ResolveExternalSourceRevisionHandler, RevalidatingGithubInstallationTokens, SourcesModule,
};
use crate::modules::workflow::{
    CancelWorkflowRunHandler, ChangeHumanTaskAssignmentHandler, CreateOntologyHandler,
    CreateWorkflowDefinitionHandler, CreateWorkflowGoalHandler, DiffOntologyRevisionsHandler,
    FlowWorkflowRunCoordinator, GetHumanTaskHandler, GetOntologyHandler,
    GetOntologyRevisionHandler, GetPlanRevisionHandler, GetWorkflowDefinitionHandler,
    GetWorkflowGoalHandler, GetWorkflowRevisionHandler, GetWorkflowRunHandler,
    GetWorkflowRunHistoryHandler, GetWorkflowRunOutputHandler, HumanTaskCoordinator,
    HumanTaskResumeWorker, HumanTaskResumeWorkerConfig, IHumanTaskRepository, IOntologyRepository,
    IWorkflowDefinitionRepository, IWorkflowGoalRepository, IWorkflowRunCoordinator,
    IWorkflowRunHistoryReader, IWorkflowRunRepository, ListHumanTasksHandler,
    ListOntologiesHandler, ListOntologyRevisionsHandler, ListWorkflowDefinitionsHandler,
    ListWorkflowGoalsHandler, ListWorkflowRevisionsHandler, ListWorkflowRunsHandler,
    PostgresHumanTaskRepository, PostgresOntologyRepository, PostgresWorkflowDefinitionRepository,
    PostgresWorkflowGoalRepository, PostgresWorkflowRunRepository, ReviseOntologyHandler,
    ReviseWorkflowDefinitionHandler, StartWorkflowRunHandler, SubmitHumanTaskHandler,
    WaitWorkflowRunHandler, WorkflowModule, WorkflowRunFlowRuntime, WorkflowRunHistoryReader,
    WorkflowRunReconciler,
};
use crate::modules::workloads::domain::repositories::IDeploymentFlowWorkloadRepository;
use crate::modules::workloads::domain::repositories::IResourceClaimRepository;
use crate::modules::workloads::domain::repositories::ISecretRotationRestartRepository;
use crate::modules::workloads::domain::repositories::IWorkloadReplicaDeploymentRepository;
use crate::modules::workloads::domain::repositories::IWorkloadReplicaEvacuationRepository;
use crate::modules::workloads::domain::repositories::IWorkloadReplicaRetirementRepository;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::domain::repositories::IWorkloadRuntimeTargetRepository;
use crate::modules::workloads::domain::services::{IDeploymentRouteUpdater, IOciArtifactResolver};
use crate::modules::workloads::{
    BindSkillWorkloadDeploymentHandler, CancelDeploymentHandler,
    CreateAgentWorkloadDeploymentHandler, CreateSourceWorkloadDeploymentHandler,
    CreateWorkloadDeploymentHandler, DeploymentFlowConfig, DeploymentFlowDependencies,
    DeploymentFlowRuntime, GetDeploymentHandler, GetWorkloadHandler, GetWorkloadLogsHandler,
    IWorkloadRuntimeControl, ListWorkloadsHandler, NodeDrainEvacuationReconciler,
    OciRegistryArtifactResolver, PostgresResourceClaimRepository, PostgresWorkloadRepository,
    ReplicaDeploymentMaterializer, ReplicaRetirementReconciler, RollbackWorkloadDeploymentHandler,
    SecretRotationRestartReconciler, StopWorkloadHandler, UnbindSkillWorkloadDeploymentHandler,
    UpdateAgentWorkloadDeploymentHandler, UpdateWorkloadDeploymentHandler,
    WorkloadRuntimeReconciler, WorkloadsModule,
};
use crate::modules::PlatformModule;
use crate::presentation::{
    ApiContractModule, ApiErrorFilter, ApiResponseInterceptor, ManagementMcpModule,
    RequestIdMiddleware, API_PREFIX,
};
use crate::server::{ControlPlane, ControlPlaneWorkers};
use crate::{
    config::{
        EventProviderKind, LogStorageProviderKind, ProcessRole, SecurityProfile,
        SecurityProviderKind,
    },
    infrastructure::{
        connect_and_migrate, postgres_health, FlowRuntimeRouter, PostgresBootstrapError,
    },
    CloudConfig,
};
use a3s_boot::{
    AuthModule, BootApplication, BootError, CqrsModule, HealthIndicatorResult, HealthModule,
    Module, ModuleRef, ProviderDefinition, ProviderToken, QueueOptions, Result, RouteDefinition,
    AUTH_PUBLIC_METADATA,
};
use a3s_event::{NatsConfig, StorageType};
use a3s_orm::PostgresExecutor;
use a3s_use_extension::MAX_BOOTSTRAP_ROOT_BYTES;
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
    #[error("could not initialize source provider access: {0}")]
    Sources(String),
    #[error("could not initialize build execution: {0}")]
    Build(String),
    #[error("could not initialize hosted Asset repositories: {0}")]
    Assets(String),
    #[error("could not initialize finite execution: {0}")]
    Execution(String),
    #[error("could not initialize Agent execution: {0}")]
    AgentExecution(String),
    #[error("could not initialize WorkflowRun execution: {0}")]
    WorkflowRun(String),
    #[error("could not initialize HumanTask workers: {0}")]
    HumanTask(String),
    #[error("could not initialize A3S Use plugin catalog: {0}")]
    Plugins(String),
    #[error("could not initialize Secret rotation restart reconciliation: {0}")]
    SecretRestart(String),
    #[error(transparent)]
    Framework(#[from] BootError),
}

pub async fn build_application(
    config: CloudConfig,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let source_resolver: Arc<dyn ISourceResolver> = Arc::new(
        GithubSourceResolver::new(Duration::from_millis(
            config.sources.github_request_timeout_ms,
        ))
        .map_err(ControlPlaneStartupError::Sources)?,
    );
    build_application_with_source_resolver(config, source_resolver).await
}

#[doc(hidden)]
pub async fn build_application_with_source_resolver(
    config: CloudConfig,
    source_resolver: Arc<dyn ISourceResolver>,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let source_webhook_verifier: Arc<dyn ISourceWebhookVerifier> = Arc::new(
        GithubWebhookVerifier::new(
            config.sources.github_webhook_secret_env.clone(),
            config.sources.github_webhook_max_body_bytes,
        )
        .map_err(ControlPlaneStartupError::Sources)?,
    );
    let postgres_url = config.postgres_url()?;
    let executor = connect_and_migrate(&postgres_url, config.postgres.max_connections).await?;
    let event_publisher = event_publisher(&config).await?;
    let vault_credentials = config.vault_credentials()?;
    let (certificate_authority, key_encryption) =
        security_providers(&config, vault_credentials.as_ref())?;
    let build_evidence_signer = build_evidence_signer(&config, vault_credentials.as_ref()).await?;
    let gateway_certificate_authority =
        gateway_certificate_authority(&config, vault_credentials.as_ref())?;
    let log_chunks = log_chunk_store(&config)?;
    let bootstrap_credential = BootstrapCredential::new(&config.bootstrap_token()?)
        .map_err(ControlPlaneStartupError::Auth)?;
    let identity = Arc::new(PostgresIdentityRepository::new(executor.clone()));
    let organizations: Arc<dyn IOrganizationRepository> = identity.clone();
    let api_tokens: Arc<dyn IApiTokenRepository> = identity.clone();
    let memberships: Arc<dyn IMembershipRepository> = identity.clone();
    let resource_grants: Arc<dyn IResourceGrantRepository> = identity.clone();
    let resource_authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository> =
        identity;
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let ontologies: Arc<dyn IOntologyRepository> =
        Arc::new(PostgresOntologyRepository::new(executor.clone()));
    let workflow_definitions: Arc<dyn IWorkflowDefinitionRepository> =
        Arc::new(PostgresWorkflowDefinitionRepository::new(executor.clone()));
    let workflow_goals: Arc<dyn IWorkflowGoalRepository> =
        Arc::new(PostgresWorkflowGoalRepository::new(executor.clone()));
    let workflow_runs: Arc<dyn IWorkflowRunRepository> =
        Arc::new(PostgresWorkflowRunRepository::new(executor.clone()));
    let forms: Arc<dyn IFormRepository> = Arc::new(PostgresFormRepository::new(executor.clone()));
    let human_tasks: Arc<dyn IHumanTaskRepository> =
        Arc::new(PostgresHumanTaskRepository::new(executor.clone()));
    let form_semantic_core: Arc<dyn IFormSemanticCore> = Arc::new(NativeFormSemanticCore::new());
    let search: Arc<dyn ISearchRepository> =
        Arc::new(PostgresSearchRepository::new(executor.clone()));
    let plugin_repository = Arc::new(PostgresPluginRegistryRepository::new(executor.clone()));
    let plugin_registries: Arc<dyn IPluginRegistryRepository> = plugin_repository.clone();
    let plugin_enrollment_authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer> =
        plugin_repository;
    let plugin_trust_roots: Arc<dyn IPluginTrustRootStore> = Arc::new(
        PluginTrustRootObjectStore::local(&config.artifacts.store_dir, MAX_BOOTSTRAP_ROOT_BYTES)
            .map_err(|error| ControlPlaneStartupError::Plugins(error.to_string()))?,
    );
    let plugin_metadata_root = std::path::absolute(
        std::path::Path::new(&config.security.state_dir).join("use-plugin-registry-metadata"),
    )
    .map_err(|error| ControlPlaneStartupError::Plugins(error.to_string()))?;
    let plugin_catalog: Arc<dyn IPluginRegistryCatalog> = Arc::new(
        A3sUsePluginRegistryCatalog::new(Arc::clone(&plugin_trust_roots), plugin_metadata_root)
            .map_err(|error| ControlPlaneStartupError::Plugins(error.to_string()))?,
    );
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let nodes: Arc<dyn INodeRepository> = node_repository.clone();
    let scheduling_nodes: Arc<dyn INodeSchedulingRepository> = node_repository.clone();
    let node_pools: Arc<dyn INodePoolRepository> = node_repository.clone();
    let draining_nodes: Arc<dyn INodeDrainRepository> = node_repository.clone();
    let node_control: Arc<dyn INodeControlRepository> = node_repository.clone();
    let node_artifacts: Arc<dyn INodeArtifactStore> = Arc::new(
        LocalNodeArtifactStore::new(&config.artifacts.store_dir, config.artifacts.max_blob_bytes)
            .map_err(ControlPlaneStartupError::NodeControl)?,
    );
    let builds: Arc<dyn IBuildRunRepository> =
        Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let executions: Arc<dyn IExecutionRepository> =
        Arc::new(PostgresExecutionRepository::new(executor.clone()));
    let execution_templates: Arc<dyn IExecutionTemplateRepository> =
        Arc::new(PostgresExecutionTemplateRepository::new(executor.clone()));
    let agents: Arc<dyn IAgentRepository> =
        Arc::new(PostgresAgentRepository::new(executor.clone()));
    let log_retention_repository: Arc<dyn ILogRetentionRepository> = node_repository.clone();
    let workload_repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let resource_claims: Arc<dyn IResourceClaimRepository> =
        Arc::new(PostgresResourceClaimRepository::new(executor.clone()));
    let workloads: Arc<dyn IWorkloadRepository> = workload_repository.clone();
    let deployment_workloads: Arc<dyn IDeploymentFlowWorkloadRepository> =
        workload_repository.clone();
    let replica_deployments: Arc<dyn IWorkloadReplicaDeploymentRepository> =
        workload_repository.clone();
    let replica_evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository> =
        workload_repository.clone();
    let replica_retirements: Arc<dyn IWorkloadReplicaRetirementRepository> =
        workload_repository.clone();
    let workload_targets: Arc<dyn IWorkloadRuntimeTargetRepository> = workload_repository.clone();
    let secret_rotation_restarts: Arc<dyn ISecretRotationRestartRepository> =
        workload_repository.clone();
    let workload_runtime_control: Arc<dyn IWorkloadRuntimeControl> = node_repository;
    let edge_repository = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let routes: Arc<dyn IEdgeRepository> = edge_repository.clone();
    let mcp_credentials: Arc<dyn IMcpCredentialLifecycleRepository> = edge_repository.clone();
    let mcp_route_policy_repository: Arc<dyn IMcpRoutePolicyRepository> = edge_repository.clone();
    let mcp_gateway_snapshots: Arc<dyn crate::modules::edge::IMcpGatewaySnapshotRepository> =
        edge_repository.clone();
    let asset_repository = Arc::new(PostgresAssetRepository::new(executor.clone()));
    let assets: Arc<dyn IAssetRepository> = asset_repository.clone();
    let asset_controls: Arc<dyn IAssetGitRepositoryControl> = asset_repository.clone();
    let mcp_profiles: Arc<dyn IMcpServiceProfileRepository> = asset_repository;
    let mcp_service_profiles = Arc::new(McpServiceProfileApplicationService::new(
        Arc::clone(&mcp_profiles),
        Arc::clone(&assets),
    ));
    let mcp_route_policies = Arc::new(McpRoutePolicyApplicationService::new(
        mcp_route_policy_repository,
        Arc::clone(&mcp_profiles),
    ));
    let asset_backup_objects =
        ImmutableObjectClient::local(&config.artifacts.store_dir, "asset-git-backups")
            .map_err(|error| ControlPlaneStartupError::Assets(error.to_string()))?;
    let asset_git_repositories: Arc<dyn IAssetGitRepository> = Arc::new(
        LocalAssetGitRepository::new(
            &config.assets.repository_dir,
            Duration::from_millis(config.assets.git_command_timeout_ms),
        )
        .and_then(|repository| {
            repository.with_backup_objects(asset_backup_objects, config.assets.backup_max_bytes)
        })
        .map_err(|error| ControlPlaneStartupError::Assets(error.to_string()))?,
    );
    let asset_git = Arc::new(
        AssetGitApplicationService::new(
            Arc::clone(&assets),
            Arc::clone(&asset_git_repositories),
            asset_controls,
            AssetGitApplicationServiceOptions {
                write_lease: Duration::from_millis(config.assets.write_lease_ms),
                default_repository_quota_bytes: config.assets.repository_quota_bytes,
                maximum_rpc_body_bytes: u64::try_from(config.assets.max_rpc_body_bytes).map_err(
                    |_| {
                        ControlPlaneStartupError::Assets(
                            "Asset Git RPC body bound exceeds u64".into(),
                        )
                    },
                )?,
            },
        )
        .map_err(ControlPlaneStartupError::Assets)?,
    );
    let asset_catalog = Arc::new(AssetCatalogApplicationService::new(
        Arc::clone(&organizations),
        Arc::clone(&assets),
        Arc::clone(&asset_git_repositories),
        Arc::clone(&node_artifacts),
    ));
    let secrets: Arc<dyn ISecretRepository> =
        Arc::new(PostgresSecretRepository::new(executor.clone()));
    let source_repository = Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let sources: Arc<dyn ISourceRevisionRepository> = source_repository.clone();
    let source_webhooks: Arc<dyn ISourceWebhookRepository> = source_repository;
    let source_subscriptions: Arc<dyn ISourceSubscriptionRepository> =
        Arc::new(PostgresSourceSubscriptionRepository::new(executor.clone()));
    let github_connections: Arc<dyn IGithubConnectionRepository> =
        Arc::new(PostgresGithubConnectionRepository::new(executor.clone()));
    let github_authorization: Arc<dyn IGithubAppAuthorizationService> =
        if config.sources.github_app_enabled {
            Arc::new(
                GithubAppClient::new(
                    Duration::from_millis(config.sources.github_request_timeout_ms),
                    config.sources.github_app_slug.clone(),
                    config.sources.github_app_client_id.clone(),
                    config.sources.github_app_client_secret_env.clone(),
                    &config.sources.github_app_callback_url,
                )
                .map_err(ControlPlaneStartupError::Sources)?,
            )
        } else {
            Arc::new(GithubAppClient::disabled())
        };
    let github_installation_client = Arc::new(if config.sources.github_app_enabled {
        GithubInstallationTokenIssuer::new(
            Duration::from_millis(config.sources.github_request_timeout_ms),
            config.sources.github_app_client_id.clone(),
            config.sources.github_app_private_key_env.clone(),
        )
        .map_err(ControlPlaneStartupError::Sources)?
    } else {
        GithubInstallationTokenIssuer::disabled()
    });
    let github_installation_tokens_raw: Arc<dyn IGithubInstallationTokenService> =
        github_installation_client.clone();
    let github_authority_provider: Arc<dyn IGithubInstallationAuthorityProvider> =
        github_installation_client;
    let github_authority_reconciler = GithubConnectionAuthorityReconciler::new(
        Arc::clone(&github_connections),
        github_authority_provider,
        Duration::from_millis(config.sources.github_authority_reconcile_interval_ms),
        Duration::from_millis(config.sources.github_authority_poll_interval_ms),
        Duration::from_millis(config.sources.github_authority_retry_initial_ms),
        Duration::from_millis(config.sources.github_authority_retry_max_ms),
        config.sources.github_authority_batch_size,
    )
    .map_err(ControlPlaneStartupError::Sources)?;
    let github_authority: Arc<dyn IGithubConnectionAuthorityService> =
        Arc::new(github_authority_reconciler.clone());
    let github_installation_tokens: Arc<dyn IGithubInstallationTokenService> = Arc::new(
        RevalidatingGithubInstallationTokens::new(github_authority, github_installation_tokens_raw),
    );
    let source_checkout: Arc<dyn ISourceCheckout> = Arc::new(
        GitSourceCheckout::new(
            &config.sources.checkout_dir,
            Duration::from_millis(config.sources.checkout_timeout_ms),
            config.sources.checkout_max_files,
            config.sources.checkout_max_bytes,
        )
        .map_err(ControlPlaneStartupError::Build)?,
    );
    let build_sources: Arc<dyn IBuildSourceResolver> = Arc::new(CloudBuildSourceResolver::new(
        Arc::clone(&sources),
        Arc::clone(&assets),
        Arc::clone(&asset_git_repositories),
    ));
    let build_inputs: Arc<dyn IBuildInputPreparer> = Arc::new(
        SourceBuildInputPreparer::new(
            source_checkout,
            Arc::clone(&github_connections),
            Arc::clone(&github_installation_tokens),
            Arc::clone(&node_artifacts),
            &config.builds.input_staging_dir,
            config.builds.input_max_entries,
            config.builds.input_max_bytes,
        )
        .map(|preparer| {
            preparer.with_hosted_assets(Arc::clone(&assets), Arc::clone(&asset_git_repositories))
        })
        .map_err(ControlPlaneStartupError::Build)?,
    );
    let build_flow_config = config
        .build_flow_config()
        .map_err(ControlPlaneStartupError::Build)?;
    let oci_build_outputs = Arc::new(
        OciBuildOutputValidator::new(
            Arc::clone(&node_artifacts),
            &config.builds.output_staging_dir,
            config.builds.output_max_bytes,
            config.builds.output_max_entries,
            config.builds.output_max_expanded_bytes,
            config.builds.oci_max_blobs,
            config.builds.oci_max_bytes,
        )
        .map_err(ControlPlaneStartupError::Build)?,
    );
    let build_outputs: Arc<dyn IBuildOutputValidator> = oci_build_outputs.clone();
    let build_publisher: Arc<dyn IBuildArtifactPublisher> = Arc::new(
        OciRegistryArtifactPublisher::new(
            Arc::clone(&oci_build_outputs),
            Duration::from_millis(config.registry.request_timeout_ms),
            config
                .registry
                .insecure_hosts
                .iter()
                .filter(|host| *host == &config.registry.publication_registry)
                .cloned(),
            OciRegistryArtifactPublisherOptions {
                registry: config.registry.publication_registry.clone(),
                repository_prefix: config.registry.publication_repository_prefix.clone(),
                credential_env: config.registry.publication_credential_env.clone(),
                allow_anonymous: config.registry.publication_allow_anonymous,
            },
        )
        .map_err(ControlPlaneStartupError::Registry)?,
    );
    let build_evidence: Arc<dyn IBuildEvidenceGenerator> = Arc::new(
        BoxBuildEvidenceGenerator::new(oci_build_outputs, build_evidence_signer)
            .map_err(ControlPlaneStartupError::Build)?,
    );
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
    let gateway_observations: Arc<dyn IGatewayObservationQueue> =
        Arc::new(FleetGatewayObservationQueue::new(Arc::clone(&node_control)));
    let deployment_route_compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: config.edge.entrypoint_address.clone(),
        management_address: config.edge.management_address.clone(),
        management_path_prefix: config.edge.management_path_prefix.clone(),
        management_auth_token_env: config.edge.management_auth_token_env.clone(),
        upstream_request_timeout_ms: config.edge.upstream_request_timeout_ms,
        certificate_directory: config.edge.certificate_directory.clone(),
        managed_state_file: config.edge.managed_state_file.clone(),
    })
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let mcp_projection_inputs = Arc::new(McpRouteProjectionInputReader::new(
        edge_repository.clone(),
        Arc::clone(&routes),
        mcp_profiles,
        Arc::clone(&workloads),
    ));
    let mcp_route_planner = McpRouteProjectionPlanner::new(
        Arc::clone(&route_targets),
        McpRouteTargetProjectionCompiler,
    );
    let mcp_projection_set_planner = Arc::new(McpGatewayProjectionSetPlanner::new(
        mcp_projection_inputs,
        McpGatewayProjectionPlanner::new(mcp_route_planner, edge_repository),
        McpGatewayProjectionAssembler,
    ));
    let mcp_node_projection_planner: Arc<
        dyn crate::modules::edge::IMcpGatewayNodeProjectionPlanner,
    > = Arc::new(McpGatewayNodeProjectionPlanner::new(
        mcp_projection_set_planner,
        McpGatewayProjectionAssembler,
    ));
    let gateway_node_desired_state_planner = GatewayNodeDesiredStatePlanner::new(
        Arc::clone(&mcp_gateway_snapshots),
        Arc::clone(&mcp_node_projection_planner),
    );
    let gateway_certificate_reconciler = GatewayCertificateReconciler::new_managed(
        Arc::clone(&routes),
        Arc::clone(&mcp_gateway_snapshots),
        gateway_node_desired_state_planner.clone(),
        Arc::clone(&route_commands),
        Arc::clone(&gateway_certificate_authority),
        deployment_route_compiler.clone(),
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        chrono_duration(config.edge.certificate_renewal_window_ms)?,
        chrono_duration(config.edge.snapshot_renewal_window_ms)?,
        chrono_duration(config.edge.command_ttl_ms)?,
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let mcp_gateway_desired_state_reconciler = McpGatewayDesiredStateReconciler::new(
        Arc::clone(&mcp_gateway_snapshots),
        mcp_node_projection_planner,
        deployment_route_compiler.clone(),
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        chrono_duration(config.edge.command_ttl_ms)?,
        chrono::Duration::hours(24),
        chrono_duration(config.edge.certificate_renewal_window_ms)?,
        chrono_duration(config.edge.command_ttl_ms)?,
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let mcp_gateway_snapshot_reconciler = McpGatewaySnapshotReconciler::new(
        Arc::clone(&mcp_gateway_snapshots),
        Arc::clone(&route_commands),
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let mcp_credential_delivery_receipt_sweeper = McpCredentialDeliveryReceiptSweeper::new(
        Arc::clone(&mcp_credentials),
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let gateway_rollout_reconciler = GatewayRolloutReconciler::new(
        Arc::clone(&routes),
        Arc::clone(&route_commands),
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let gateway_replica_recovery_reconciler = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&routes),
        gateway_observations,
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        chrono_duration(config.edge.command_ttl_ms)?,
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let gateway_rollout_rollback_reconciler = GatewayRolloutRollbackReconciler::new_managed(
        Arc::clone(&routes),
        Arc::clone(&mcp_gateway_snapshots),
        gateway_node_desired_state_planner.clone(),
        GatewayRolloutRollbackCompiler::new(
            deployment_route_compiler.clone(),
            chrono_duration(config.edge.command_ttl_ms)?,
            chrono::Duration::hours(24),
        )
        .map_err(ControlPlaneStartupError::Edge)?,
        Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Edge)?;
    let deployment_route_updates: Arc<dyn IDeploymentRouteUpdater> = Arc::new(
        EdgeDeploymentRouteUpdater::new_managed(
            Arc::clone(&routes),
            Arc::clone(&mcp_gateway_snapshots),
            Arc::clone(&node_control),
            Arc::clone(&route_commands),
            deployment_route_compiler,
            gateway_node_desired_state_planner.clone(),
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
        DeploymentFlowDependencies::new(
            deployment_workloads,
            Arc::clone(&resource_claims),
            artifacts,
            scheduling_nodes,
            Arc::clone(&node_control),
            deployment_route_updates,
        ),
        chrono_duration(config.fleet.heartbeat_timeout_ms)
            .map_err(|error| ControlPlaneStartupError::NodeControl(error.to_string()))?,
        deployment_flow_config,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let build_runtime = BuildFlowRuntime::new(
        BuildFlowRuntimeDependencies {
            builds: Arc::clone(&builds),
            sources: build_sources,
            inputs: build_inputs,
            outputs: build_outputs,
            publisher: build_publisher,
            evidence: build_evidence,
            nodes: Arc::clone(&nodes),
            node_control: Arc::clone(&node_control),
        },
        build_flow_config,
    );
    let execution_runtime = ExecutionFlowRuntime::new(
        ExecutionFlowRuntimeDependencies {
            executions: Arc::clone(&executions),
            nodes: Arc::clone(&nodes),
            node_control: Arc::clone(&node_control),
        },
        config
            .execution_flow_config()
            .map_err(ControlPlaneStartupError::Execution)?,
    );
    let agent_execution_runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents: Arc::clone(&agents),
            workload_targets: Arc::clone(&workload_targets),
            node_control: Arc::clone(&node_control),
        },
        config
            .agent_execution_flow_config()
            .map_err(ControlPlaneStartupError::AgentExecution)?,
    );
    let flow_runtime = FlowRuntimeRouter::new(
        Arc::new(deployment_runtime),
        Arc::new(build_runtime),
        Arc::new(execution_runtime),
        Arc::new(agent_execution_runtime),
        Arc::new(WorkflowRunFlowRuntime),
    );
    let operation_interval = Duration::from_millis(config.operations.reconcile_interval_ms);
    let operation_lease = Duration::from_millis(config.operations.lease_ms);
    let flow = crate::infrastructure::connect_flow(
        &postgres_url,
        Arc::new(flow_runtime),
        QueueOptions::new()
            .with_poll_interval(operation_interval)
            .with_lease_duration(operation_lease),
    )
    .await?;
    let workflow_execution_environments: Arc<dyn IEnvironmentRepository> = projects.clone();
    let workflow_execution_port: Arc<dyn IWorkflowExecutionPort> =
        Arc::new(WorkflowExecutionApplicationService::new(
            workflow_execution_environments,
            Arc::clone(&execution_templates),
            Arc::clone(&executions),
        ));
    let workflow_run_coordinator: Arc<dyn IWorkflowRunCoordinator> = Arc::new(
        FlowWorkflowRunCoordinator::with_executions(flow.engine(), workflow_execution_port),
    );
    let workflow_run_history: Arc<dyn IWorkflowRunHistoryReader> =
        Arc::new(WorkflowRunHistoryReader::new(flow.engine()));
    let workflow_run_reconciler = WorkflowRunReconciler::new(
        Arc::clone(&workflow_runs),
        workflow_run_coordinator,
        operation_interval,
        100,
    )
    .map_err(ControlPlaneStartupError::WorkflowRun)?;
    let human_task_coordinator = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        Arc::clone(&forms),
        Arc::clone(&human_tasks),
        flow.engine(),
        Duration::from_millis(config.human_tasks.coordination_poll_interval_ms),
        config.human_tasks.coordination_batch_size,
    )
    .map_err(ControlPlaneStartupError::HumanTask)?;
    let human_task_resume_worker = HumanTaskResumeWorker::new(
        Arc::clone(&human_tasks),
        flow.engine(),
        HumanTaskResumeWorkerConfig {
            batch_size: config.human_tasks.resume_batch_size,
            poll_interval: Duration::from_millis(config.human_tasks.resume_poll_interval_ms),
            lease_duration: Duration::from_millis(config.human_tasks.resume_lease_ms),
            flow_operation_timeout: Duration::from_millis(
                config.human_tasks.flow_operation_timeout_ms,
            ),
            initial_backoff: Duration::from_millis(config.human_tasks.retry_initial_ms),
            maximum_backoff: Duration::from_millis(config.human_tasks.retry_max_ms),
        },
    )
    .map_err(ControlPlaneStartupError::HumanTask)?;
    let run_node_control = matches!(config.server.role, ProcessRole::All | ProcessRole::Api);
    let node_control_server = if run_node_control {
        let api = NodeControlApi::new(
            Arc::clone(&nodes),
            Arc::clone(&node_control),
            Arc::clone(&agents),
            Arc::clone(&node_artifacts),
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
            Duration::from_millis(config.artifacts.transfer_timeout_ms),
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
    let build_run_reconciler = BuildRunReconciler::with_schedule(
        Arc::clone(&builds),
        Arc::clone(&operation_repository),
        Duration::from_millis(config.builds.reconcile_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Build)?;
    let execution_reconciler = ExecutionReconciler::with_schedule(
        Arc::clone(&executions),
        Arc::clone(&operation_repository),
        Duration::from_millis(config.executions.reconcile_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::Execution)?;
    let agent_execution_reconciler = AgentExecutionReconciler::with_schedule(
        Arc::clone(&agents),
        Arc::clone(&operation_repository),
        Duration::from_millis(config.executions.reconcile_interval_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::AgentExecution)?;
    let operation_engine = Arc::new(FlowOperationEngine::new(flow.engine()));
    let operation_reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            operation_repository.clone(),
            operation_engine,
        )),
        operation_interval,
        100,
    );
    let operation_coordinator = crate::infrastructure::FlowOperationCoordinator::new(
        operation_reconciler,
        &flow,
        operation_interval,
        operation_lease,
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
    let node_drain_evacuation_reconciler = NodeDrainEvacuationReconciler::new(
        draining_nodes,
        Arc::clone(&node_pools),
        replica_evacuations,
        Arc::clone(&resource_claims),
        Duration::from_millis(config.deployments.reconcile_interval_ms),
        100,
        100,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let replica_retirement_reconciler = ReplicaRetirementReconciler::new(
        replica_retirements,
        Arc::clone(&workload_runtime_control),
        Arc::clone(&resource_claims),
        Duration::from_millis(config.deployments.reconcile_interval_ms),
        Duration::from_millis(config.deployments.command_ttl_ms),
        Duration::from_millis(config.deployments.runtime_stop_timeout_ms),
        Duration::from_millis(config.deployments.cleanup_timeout_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let workload_reconciler = WorkloadRuntimeReconciler::new(
        workload_targets,
        workload_runtime_control,
        resource_claims,
        Duration::from_millis(config.deployments.reconcile_interval_ms),
        Duration::from_millis(config.deployments.command_ttl_ms),
        Duration::from_millis(config.deployments.runtime_apply_timeout_ms),
        100,
    )
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let replica_deployment_materializer = ReplicaDeploymentMaterializer::new(
        replica_deployments,
        Duration::from_millis(config.deployments.reconcile_interval_ms),
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
            memberships,
            resource_grants,
            resource_authorization_decisions,
            projects: projects.clone(),
            environments: projects,
            ontologies,
            workflow_definitions,
            workflow_goals,
            workflow_runs,
            human_tasks,
            workflow_run_history,
            forms,
            form_semantic_core,
            search,
            plugin_registries,
            plugin_enrollment_authorizer,
            plugin_trust_roots,
            plugin_catalog,
            asset_catalog,
            mcp_service_profiles,
            mcp_route_policies,
            asset_git,
            assets,
            workloads,
            builds,
            executions,
            execution_templates,
            agents,
            routes,
            mcp_credentials,
            secrets,
            sources,
            source_webhooks,
            source_subscriptions,
            github_connections,
            github_authorization,
            github_installation_tokens,
            source_resolver,
            source_webhook_verifier,
            secret_encryption: Arc::clone(&key_encryption),
            route_targets,
            route_commands,
            mcp_gateway_snapshots: Some(mcp_gateway_snapshots),
            gateway_node_desired_state_planner: Some(gateway_node_desired_state_planner),
            domain_verifier,
            gateway_projector,
            operations: operation_repository,
            nodes,
            node_pools,
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
            run_operations.then_some(build_run_reconciler),
            run_operations.then_some(execution_reconciler),
            run_operations.then_some(agent_execution_reconciler),
            run_operations.then_some(workflow_run_reconciler),
            run_operations.then_some(human_task_coordinator),
            run_operations.then_some(human_task_resume_worker),
            run_operations.then_some(github_authority_reconciler),
            run_operations.then_some(operation_coordinator),
            run_operations.then_some(gateway_certificate_reconciler),
            run_operations.then_some(mcp_gateway_desired_state_reconciler),
            run_operations.then_some(mcp_gateway_snapshot_reconciler),
            run_operations.then_some(mcp_credential_delivery_receipt_sweeper),
            run_operations.then_some(gateway_rollout_reconciler),
            run_operations.then_some(gateway_replica_recovery_reconciler),
            run_operations.then_some(gateway_rollout_rollback_reconciler),
            run_operations.then_some(secret_rotation_restart_reconciler),
            run_operations.then_some(node_drain_evacuation_reconciler),
            run_operations.then_some(replica_deployment_materializer),
            run_operations.then_some(replica_retirement_reconciler),
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
    memberships: Arc<dyn IMembershipRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
    resource_authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
    workflow_definitions: Arc<dyn IWorkflowDefinitionRepository>,
    workflow_goals: Arc<dyn IWorkflowGoalRepository>,
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
    human_tasks: Arc<dyn IHumanTaskRepository>,
    workflow_run_history: Arc<dyn IWorkflowRunHistoryReader>,
    forms: Arc<dyn IFormRepository>,
    form_semantic_core: Arc<dyn IFormSemanticCore>,
    search: Arc<dyn ISearchRepository>,
    plugin_registries: Arc<dyn IPluginRegistryRepository>,
    plugin_enrollment_authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer>,
    plugin_trust_roots: Arc<dyn IPluginTrustRootStore>,
    plugin_catalog: Arc<dyn IPluginRegistryCatalog>,
    asset_catalog: Arc<AssetCatalogApplicationService>,
    mcp_service_profiles: Arc<McpServiceProfileApplicationService>,
    mcp_route_policies: Arc<McpRoutePolicyApplicationService>,
    asset_git: Arc<AssetGitApplicationService>,
    assets: Arc<dyn IAssetRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    builds: Arc<dyn IBuildRunRepository>,
    executions: Arc<dyn IExecutionRepository>,
    execution_templates: Arc<dyn IExecutionTemplateRepository>,
    agents: Arc<dyn IAgentRepository>,
    routes: Arc<dyn IEdgeRepository>,
    mcp_credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
    secrets: Arc<dyn ISecretRepository>,
    sources: Arc<dyn ISourceRevisionRepository>,
    source_webhooks: Arc<dyn ISourceWebhookRepository>,
    source_subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    github_connections: Arc<dyn IGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    source_resolver: Arc<dyn ISourceResolver>,
    source_webhook_verifier: Arc<dyn ISourceWebhookVerifier>,
    secret_encryption: Arc<dyn ISecretEncryptionService>,
    route_targets: Arc<dyn IRouteTargetReader>,
    route_commands: Arc<dyn IGatewayCommandQueue>,
    mcp_gateway_snapshots: Option<Arc<dyn crate::modules::edge::IMcpGatewaySnapshotRepository>>,
    gateway_node_desired_state_planner: Option<GatewayNodeDesiredStatePlanner>,
    domain_verifier: Arc<dyn IDomainOwnershipVerifier>,
    gateway_projector: Arc<dyn IGatewayAcknowledgementProjector>,
    operations: Arc<dyn IOperationRepository>,
    nodes: Arc<dyn INodeRepository>,
    node_pools: Arc<dyn INodePoolRepository>,
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
        memberships,
        resource_grants,
        resource_authorization_decisions,
        projects,
        environments,
        ontologies,
        workflow_definitions,
        workflow_goals,
        workflow_runs,
        human_tasks,
        workflow_run_history,
        forms,
        form_semantic_core,
        search,
        plugin_registries,
        plugin_enrollment_authorizer,
        plugin_trust_roots,
        plugin_catalog,
        asset_catalog,
        mcp_service_profiles,
        mcp_route_policies,
        asset_git,
        assets,
        workloads,
        builds,
        executions,
        execution_templates,
        agents,
        routes,
        mcp_credentials,
        secrets,
        sources,
        source_webhooks,
        source_subscriptions,
        github_connections,
        github_authorization,
        github_installation_tokens,
        source_resolver,
        source_webhook_verifier,
        secret_encryption,
        route_targets,
        route_commands,
        mcp_gateway_snapshots,
        gateway_node_desired_state_planner,
        domain_verifier,
        gateway_projector,
        operations,
        nodes,
        node_pools,
        node_control,
        log_chunks,
        certificate_authority,
        bootstrap_credential,
        readiness,
    } = dependencies;
    let operation_resource_access = Arc::new(OperationResourceAccessResolver::new(
        Arc::clone(&workloads),
        Arc::clone(&builds),
        Arc::clone(&executions),
        Arc::clone(&agents),
        Arc::clone(&workflow_runs),
    ));
    let project_organizations = Arc::clone(&organizations);
    let environment_projects = Arc::clone(&projects);
    let create_ontology_projects = Arc::clone(&projects);
    let create_ontologies = Arc::clone(&ontologies);
    let revise_ontologies = Arc::clone(&ontologies);
    let get_ontologies = Arc::clone(&ontologies);
    let list_ontologies = Arc::clone(&ontologies);
    let get_ontology_revisions = Arc::clone(&ontologies);
    let list_ontology_revisions = Arc::clone(&ontologies);
    let diff_ontology_revisions = Arc::clone(&ontologies);
    let create_workflow_projects = Arc::clone(&projects);
    let create_workflow_definitions = Arc::clone(&workflow_definitions);
    let revise_workflow_definitions = Arc::clone(&workflow_definitions);
    let get_workflow_definitions = Arc::clone(&workflow_definitions);
    let list_workflow_definitions = Arc::clone(&workflow_definitions);
    let get_workflow_revisions = Arc::clone(&workflow_definitions);
    let list_workflow_revisions = Arc::clone(&workflow_definitions);
    let create_workflow_goal_projects = Arc::clone(&projects);
    let create_workflow_goal_environments = Arc::clone(&environments);
    let create_goal_workflows = Arc::clone(&workflow_definitions);
    let create_goal_ontologies = Arc::clone(&ontologies);
    let create_workflow_goals = Arc::clone(&workflow_goals);
    let get_workflow_goals = Arc::clone(&workflow_goals);
    let list_workflow_goals = Arc::clone(&workflow_goals);
    let get_plan_revisions = Arc::clone(&workflow_goals);
    let start_workflow_run_goals = Arc::clone(&workflow_goals);
    let start_workflow_run_workflows = Arc::clone(&workflow_definitions);
    let start_workflow_runs = Arc::clone(&workflow_runs);
    let cancel_workflow_runs = Arc::clone(&workflow_runs);
    let get_workflow_runs = Arc::clone(&workflow_runs);
    let list_workflow_runs = Arc::clone(&workflow_runs);
    let wait_workflow_runs = Arc::clone(&workflow_runs);
    let get_workflow_run_outputs = Arc::clone(&workflow_runs);
    let get_workflow_run_history_runs = workflow_runs;
    let change_human_task_assignments = Arc::clone(&human_tasks);
    let submit_human_tasks = Arc::clone(&human_tasks);
    let submit_human_task_forms = Arc::clone(&forms);
    let submit_human_task_semantic_core = Arc::clone(&form_semantic_core);
    let get_human_tasks = Arc::clone(&human_tasks);
    let list_human_tasks = human_tasks;
    let create_form_projects = Arc::clone(&projects);
    let create_form_drafts = Arc::clone(&forms);
    let revise_form_drafts = Arc::clone(&forms);
    let publish_form_releases = Arc::clone(&forms);
    let get_form_drafts = Arc::clone(&forms);
    let list_form_drafts = Arc::clone(&forms);
    let get_form_releases = Arc::clone(&forms);
    let list_form_releases = forms;
    let agent_conversation_environments = Arc::clone(&environments);
    let workload_environments = Arc::clone(&environments);
    let source_workload_environments = Arc::clone(&environments);
    let agent_workload_environments = Arc::clone(&environments);
    let domain_environments = Arc::clone(&environments);
    let gateway_scope_environments = Arc::clone(&environments);
    let mcp_credential_environments = Arc::clone(&environments);
    let secret_environments = Arc::clone(&environments);
    let source_environments = Arc::clone(&environments);
    let source_query_environments = Arc::clone(&environments);
    let create_subscription_environments = Arc::clone(&environments);
    let deactivate_subscription_environments = Arc::clone(&environments);
    let subscription_query_environments = Arc::clone(&environments);
    let github_connection_organizations = Arc::clone(&organizations);
    let create_workloads = Arc::clone(&workloads);
    let source_create_workloads = Arc::clone(&workloads);
    let agent_create_workloads = Arc::clone(&workloads);
    let workload_node_pools = Arc::clone(&node_pools);
    let source_workload_node_pools = Arc::clone(&node_pools);
    let agent_workload_node_pools = Arc::clone(&node_pools);
    let agent_update_workloads = Arc::clone(&workloads);
    let bind_skill_workloads = Arc::clone(&workloads);
    let unbind_skill_workloads = Arc::clone(&workloads);
    let workload_secrets = Arc::clone(&secrets);
    let source_workload_secrets = Arc::clone(&secrets);
    let agent_create_workload_secrets = Arc::clone(&secrets);
    let agent_update_workload_secrets = Arc::clone(&secrets);
    let bind_skill_workload_secrets = Arc::clone(&secrets);
    let unbind_skill_workload_secrets = Arc::clone(&secrets);
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
    let list_api_tokens = Arc::clone(&api_tokens);
    let get_api_tokens = Arc::clone(&api_tokens);
    let create_memberships = Arc::clone(&memberships);
    let change_memberships = Arc::clone(&memberships);
    let revoke_memberships = Arc::clone(&memberships);
    let list_memberships = Arc::clone(&memberships);
    let get_memberships = Arc::clone(&memberships);
    let create_resource_grants = Arc::clone(&resource_grants);
    let resource_grant_projects = Arc::clone(&projects);
    let resource_grant_environments = Arc::clone(&environments);
    let resource_grant_nodes = Arc::clone(&nodes);
    let revoke_resource_grants = Arc::clone(&resource_grants);
    let list_resource_grants = Arc::clone(&resource_grants);
    let get_resource_grants = Arc::clone(&resource_grants);
    let query_organizations = Arc::clone(&organizations);
    let query_projects = Arc::clone(&projects);
    let list_environment_projects = Arc::clone(&projects);
    let query_environments = Arc::clone(&environments);
    let create_assets = Arc::clone(&asset_catalog);
    let archive_assets = Arc::clone(&asset_catalog);
    let create_asset_releases = Arc::clone(&asset_catalog);
    let yank_asset_releases = Arc::clone(&asset_catalog);
    let list_assets = Arc::clone(&asset_catalog);
    let get_assets = Arc::clone(&asset_catalog);
    let list_asset_releases = Arc::clone(&asset_catalog);
    let get_asset_releases = Arc::clone(&asset_catalog);
    let bind_mcp_service_profiles = Arc::clone(&mcp_service_profiles);
    let get_mcp_service_profiles = mcp_service_profiles;
    let create_mcp_route_policies = Arc::clone(&mcp_route_policies);
    let revise_mcp_route_policies = Arc::clone(&mcp_route_policies);
    let list_mcp_route_policies = Arc::clone(&mcp_route_policies);
    let get_mcp_route_policies = mcp_route_policies;
    let agent_create_assets = Arc::clone(&assets);
    let agent_update_assets = Arc::clone(&assets);
    let agent_execution_assets = Arc::clone(&assets);
    let bind_skill_assets = assets;
    let select_asset_releases = asset_catalog;
    let enrollment_nodes = Arc::clone(&nodes);
    let rotation_nodes = Arc::clone(&nodes);
    let state_nodes = Arc::clone(&nodes);
    let get_nodes = Arc::clone(&nodes);
    let manage_node_pools = Arc::clone(&node_pools);
    let get_node_pools = Arc::clone(&node_pools);
    let list_node_pools = node_pools;
    let gateway_scope_nodes = Arc::clone(&nodes);
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
    let revoke_domain_claims = Arc::clone(&routes);
    let create_gateway_scopes = Arc::clone(&routes);
    let publish_routes = Arc::clone(&routes);
    let list_domain_claims = Arc::clone(&routes);
    let get_domain_claims = Arc::clone(&routes);
    let list_gateway_certificates = Arc::clone(&routes);
    let list_gateway_scopes = Arc::clone(&routes);
    let list_routes = Arc::clone(&routes);
    let get_routes = routes;
    let create_mcp_credentials = Arc::clone(&mcp_credentials);
    let rotate_mcp_credentials = Arc::clone(&mcp_credentials);
    let revoke_mcp_credentials = Arc::clone(&mcp_credentials);
    let list_mcp_credentials = Arc::clone(&mcp_credentials);
    let get_mcp_credentials = mcp_credentials;
    let create_secrets = Arc::clone(&secrets);
    let rotate_secrets = Arc::clone(&secrets);
    let revoke_secret_versions = Arc::clone(&secrets);
    let list_secrets = Arc::clone(&secrets);
    let get_secrets = secrets;
    let accept_sources = Arc::clone(&sources);
    let source_workload_sources = Arc::clone(&sources);
    let list_sources = sources;
    let cancel_builds = Arc::clone(&builds);
    let retry_builds = Arc::clone(&builds);
    let list_builds = Arc::clone(&builds);
    let get_builds = Arc::clone(&builds);
    let get_build_evidence = Arc::clone(&builds);
    let get_build_logs = Arc::clone(&builds);
    let agent_create_builds = Arc::clone(&builds);
    let agent_update_builds = Arc::clone(&builds);
    let agent_execution_builds = Arc::clone(&builds);
    let source_workload_builds = builds;
    let execution_environments = Arc::clone(&environments);
    let create_execution_template_projects = Arc::clone(&projects);
    let list_execution_template_projects = Arc::clone(&projects);
    let create_execution_templates = Arc::clone(&execution_templates);
    let list_execution_templates = Arc::clone(&execution_templates);
    let get_execution_templates = execution_templates;
    let create_executions = Arc::clone(&executions);
    let cancel_executions = Arc::clone(&executions);
    let list_executions = Arc::clone(&executions);
    let get_executions = executions;
    let create_agent_conversations = Arc::clone(&agents);
    let start_agent_executions = Arc::clone(&agents);
    let cancel_agent_executions = Arc::clone(&agents);
    let append_agent_execution_events = Arc::clone(&agents);
    let get_agent_conversations = Arc::clone(&agents);
    let list_agent_conversations = Arc::clone(&agents);
    let get_agent_executions = Arc::clone(&agents);
    let get_agent_execution_change_sets = Arc::clone(&agents);
    let list_agent_executions = Arc::clone(&agents);
    let get_agent_execution_events = agents;
    let accept_source_webhooks = source_webhooks;
    let create_source_subscriptions = Arc::clone(&source_subscriptions);
    let deactivate_source_subscriptions = Arc::clone(&source_subscriptions);
    let list_source_subscriptions = source_subscriptions;
    let begin_github_connections = Arc::clone(&github_connections);
    let prepare_github_connections = Arc::clone(&github_connections);
    let complete_github_connections = Arc::clone(&github_connections);
    let accept_webhook_connections = Arc::clone(&github_connections);
    let reconcile_github_connections = Arc::clone(&github_connections);
    let create_subscription_connections = Arc::clone(&github_connections);
    let resolve_github_connections = Arc::clone(&github_connections);
    let get_github_connections = github_connections;
    let begin_github_authorization = Arc::clone(&github_authorization);
    let prepare_github_authorization = Arc::clone(&github_authorization);
    let complete_github_authorization = github_authorization;
    let source_policy = Arc::new(
        SourceRepositoryPolicy::github(
            &config.sources.allowed_repositories,
            &config.sources.denied_repositories,
        )
        .map_err(BootError::Internal)?,
    );
    let subscription_source_policy = Arc::clone(&source_policy);
    let create_secret_encryption = Arc::clone(&secret_encryption);
    let rotate_secret_encryption = Arc::clone(&secret_encryption);
    let create_mcp_credential_encryption = Arc::clone(&secret_encryption);
    let rotate_mcp_credential_encryption = secret_encryption;
    let mcp_credential_issuer: Arc<dyn IMcpCredentialIssuer> = Arc::new(McpCredentialIssuer::new());
    let rotate_mcp_credential_issuer = Arc::clone(&mcp_credential_issuer);
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
        managed_state_file: config.edge.managed_state_file.clone(),
    })
    .map_err(BootError::Internal)?;
    let publish_route_handler = match (mcp_gateway_snapshots, gateway_node_desired_state_planner) {
        (Some(mcp_gateway_snapshots), Some(gateway_node_desired_state_planner)) => {
            PublishRouteHandler::new_managed(
                publish_routes,
                mcp_gateway_snapshots,
                route_targets,
                route_commands,
                route_compiler,
                gateway_node_desired_state_planner,
                chrono_duration(config.edge.command_ttl_ms)?,
            )
        }
        (None, None) => PublishRouteHandler::new(
            publish_routes,
            route_targets,
            route_commands,
            route_compiler,
            chrono_duration(config.edge.command_ttl_ms)?,
        ),
        _ => Err("managed Gateway publication dependencies are incomplete".into()),
    }
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
                .bearer(ApiTokenVerifier::new(
                    Arc::clone(&api_tokens),
                    Arc::clone(&resource_grants),
                ))
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
                .command_handler::<crate::modules::identity::CreateServiceMembership, _>(
                    CreateServiceMembershipHandler::new(create_memberships),
                )
                .command_handler::<crate::modules::identity::ChangeMembershipRole, _>(
                    ChangeMembershipRoleHandler::new(change_memberships),
                )
                .command_handler::<crate::modules::identity::RevokeMembership, _>(
                    RevokeMembershipHandler::new(revoke_memberships),
                )
                .command_handler::<crate::modules::identity::CreateResourceGrant, _>(
                    CreateResourceGrantHandler::new(
                        create_resource_grants,
                        resource_grant_projects,
                        resource_grant_environments,
                        resource_grant_nodes,
                    ),
                )
                .command_handler::<crate::modules::identity::RevokeResourceGrant, _>(
                    RevokeResourceGrantHandler::new(revoke_resource_grants),
                )
                .command_handler::<crate::modules::projects::CreateProject, _>(
                    CreateProjectHandler::new(project_organizations, projects),
                )
                .command_handler::<crate::modules::projects::CreateEnvironment, _>(
                    CreateEnvironmentHandler::new(environment_projects, environments),
                )
                .command_handler::<crate::modules::workflow::CreateOntology, _>(
                    CreateOntologyHandler::new(create_ontology_projects, create_ontologies),
                )
                .command_handler::<crate::modules::workflow::ReviseOntology, _>(
                    ReviseOntologyHandler::new(revise_ontologies),
                )
                .command_handler::<crate::modules::workflow::CreateWorkflowDefinition, _>(
                    CreateWorkflowDefinitionHandler::new(
                        create_workflow_projects,
                        create_workflow_definitions,
                    ),
                )
                .command_handler::<crate::modules::workflow::ReviseWorkflowDefinition, _>(
                    ReviseWorkflowDefinitionHandler::new(revise_workflow_definitions),
                )
                .command_handler::<crate::modules::workflow::CreateWorkflowGoal, _>(
                    CreateWorkflowGoalHandler::new(
                        create_workflow_goal_projects,
                        create_workflow_goal_environments,
                        create_goal_workflows,
                        create_goal_ontologies,
                        create_workflow_goals,
                    ),
                )
                .command_handler::<crate::modules::workflow::StartWorkflowRun, _>(
                    StartWorkflowRunHandler::new(
                        start_workflow_run_goals,
                        start_workflow_run_workflows,
                        start_workflow_runs,
                    ),
                )
                .command_handler::<crate::modules::workflow::CancelWorkflowRun, _>(
                    CancelWorkflowRunHandler::new(cancel_workflow_runs),
                )
                .command_handler::<crate::modules::workflow::ChangeHumanTaskAssignment, _>(
                    ChangeHumanTaskAssignmentHandler::new(change_human_task_assignments),
                )
                .command_handler::<crate::modules::workflow::SubmitHumanTask, _>(
                    SubmitHumanTaskHandler::new(
                        submit_human_tasks,
                        submit_human_task_forms,
                        submit_human_task_semantic_core,
                        resource_authorization_decisions,
                    ),
                )
                .command_handler::<crate::modules::forms::CreateFormDraft, _>(
                    CreateFormDraftHandler::new(create_form_projects, create_form_drafts),
                )
                .command_handler::<crate::modules::forms::ReviseFormDraft, _>(
                    ReviseFormDraftHandler::new(revise_form_drafts),
                )
                .command_handler::<crate::modules::forms::PublishFormRelease, _>(
                    PublishFormReleaseHandler::new(publish_form_releases, form_semantic_core),
                )
                .command_handler::<crate::modules::assets::CreateAsset, _>(
                    CreateAssetHandler::new(create_assets),
                )
                .command_handler::<crate::modules::assets::ArchiveAsset, _>(
                    ArchiveAssetHandler::new(archive_assets),
                )
                .command_handler::<crate::modules::assets::CreateAssetRelease, _>(
                    CreateAssetReleaseHandler::new(create_asset_releases),
                )
                .command_handler::<crate::modules::assets::BindMcpServiceProfile, _>(
                    BindMcpServiceProfileHandler::new(bind_mcp_service_profiles),
                )
                .command_handler::<crate::modules::assets::YankAssetRelease, _>(
                    YankAssetReleaseHandler::new(yank_asset_releases),
                )
                .command_handler::<crate::modules::assets::ReceiveAssetGitPack, _>(
                    ReceiveAssetGitPackHandler::new(Arc::clone(&asset_git)),
                )
                .command_handler::<crate::modules::assets::BackupAssetGitRepository, _>(
                    BackupAssetGitRepositoryHandler::new(Arc::clone(&asset_git)),
                )
                .command_handler::<crate::modules::assets::RestoreAssetGitRepository, _>(
                    RestoreAssetGitRepositoryHandler::new(Arc::clone(&asset_git)),
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
                .command_handler::<crate::modules::sources::ResolveExternalSourceRevision, _>(
                    ResolveExternalSourceRevisionHandler::new(
                        source_environments,
                        accept_sources,
                        resolve_github_connections,
                        github_installation_tokens,
                        source_resolver,
                        source_policy,
                    ),
                )
                .command_handler::<crate::modules::sources::AcceptSourceWebhookDelivery, _>(
                    AcceptSourceWebhookDeliveryHandler::new(
                        accept_source_webhooks,
                        accept_webhook_connections,
                    ),
                )
                .command_handler::<crate::modules::sources::ReconcileGithubConnectionLifecycle, _>(
                    ReconcileGithubConnectionLifecycleHandler::new(reconcile_github_connections),
                )
                .command_handler::<crate::modules::sources::CreateGithubRepositorySubscription, _>(
                    CreateGithubRepositorySubscriptionHandler::new(
                        create_subscription_environments,
                        create_subscription_connections,
                        create_source_subscriptions,
                        subscription_source_policy,
                    ),
                )
                .command_handler::<crate::modules::sources::DeactivateGithubRepositorySubscription, _>(
                    DeactivateGithubRepositorySubscriptionHandler::new(
                        deactivate_subscription_environments,
                        deactivate_source_subscriptions,
                    ),
                )
                .command_handler::<crate::modules::sources::BeginGithubConnection, _>(
                    BeginGithubConnectionHandler::new(
                        github_connection_organizations,
                        begin_github_connections,
                        begin_github_authorization,
                        chrono_duration(config.sources.github_connection_state_ttl_ms)?,
                    )
                    .map_err(BootError::Internal)?,
                )
                .command_handler::<crate::modules::sources::PrepareGithubConnectionOauth, _>(
                    PrepareGithubConnectionOauthHandler::new(
                        prepare_github_connections,
                        prepare_github_authorization,
                    ),
                )
                .command_handler::<crate::modules::sources::CompleteGithubConnection, _>(
                    CompleteGithubConnectionHandler::new(
                        complete_github_connections,
                        complete_github_authorization,
                    ),
                )
                .command_handler::<crate::modules::workloads::CreateWorkloadDeployment, _>(
                    CreateWorkloadDeploymentHandler::new(
                        workload_environments,
                        create_workloads,
                        workload_secrets,
                        workload_node_pools,
                    ),
                )
                .command_handler::<crate::modules::workloads::CreateSourceWorkloadDeployment, _>(
                    CreateSourceWorkloadDeploymentHandler::new(
                        source_workload_environments,
                        source_workload_sources,
                        source_workload_builds,
                        source_create_workloads,
                        source_workload_secrets,
                        source_workload_node_pools,
                    ),
                )
                .command_handler::<crate::modules::workloads::CreateAgentWorkloadDeployment, _>(
                    CreateAgentWorkloadDeploymentHandler::new(
                        agent_workload_environments,
                        agent_create_assets,
                        agent_create_builds,
                        agent_create_workloads,
                        agent_create_workload_secrets,
                        agent_workload_node_pools,
                    ),
                )
                .command_handler::<crate::modules::workloads::UpdateAgentWorkloadDeployment, _>(
                    UpdateAgentWorkloadDeploymentHandler::new(
                        agent_update_assets,
                        agent_update_builds,
                        agent_update_workloads,
                        agent_update_workload_secrets,
                    ),
                )
                .command_handler::<crate::modules::workloads::BindSkillWorkloadDeployment, _>(
                    BindSkillWorkloadDeploymentHandler::new(
                        bind_skill_assets,
                        bind_skill_workloads,
                        bind_skill_workload_secrets,
                    ),
                )
                .command_handler::<crate::modules::workloads::UnbindSkillWorkloadDeployment, _>(
                    UnbindSkillWorkloadDeploymentHandler::new(
                        unbind_skill_workloads,
                        unbind_skill_workload_secrets,
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
                .command_handler::<crate::modules::artifacts::CancelBuildRun, _>(
                    CancelBuildRunHandler::new(cancel_builds),
                )
                .command_handler::<crate::modules::artifacts::RetryBuildRun, _>(
                    RetryBuildRunHandler::new(retry_builds),
                )
                .command_handler::<crate::modules::executions::CreateExecutionCommand, _>(
                    CreateExecutionHandler::new(execution_environments, create_executions),
                )
                .command_handler::<crate::modules::executions::CreateExecutionTemplateCommand, _>(
                    CreateExecutionTemplateHandler::new(
                        create_execution_template_projects,
                        create_execution_templates,
                    ),
                )
                .command_handler::<crate::modules::executions::CancelExecution, _>(
                    CancelExecutionHandler::new(cancel_executions),
                )
                .command_handler::<crate::modules::agents::CreateAgentConversation, _>(
                    CreateAgentConversationHandler::new(
                        agent_conversation_environments,
                        create_agent_conversations,
                    ),
                )
                .command_handler::<crate::modules::agents::StartAgentExecution, _>(
                    StartAgentExecutionHandler::new(
                        start_agent_executions,
                        agent_execution_assets,
                        agent_execution_builds,
                    ),
                )
                .command_handler::<crate::modules::agents::CancelAgentExecution, _>(
                    CancelAgentExecutionHandler::new(cancel_agent_executions),
                )
                .command_handler::<crate::modules::agents::AppendAgentExecutionEvents, _>(
                    AppendAgentExecutionEventsHandler::new(append_agent_execution_events),
                )
                .command_handler::<crate::modules::edge::CreateDomainClaim, _>(
                    CreateDomainClaimHandler::new(domain_environments, create_domain_claims),
                )
                .command_handler::<crate::modules::edge::VerifyDomainClaim, _>(
                    VerifyDomainClaimHandler::new(verify_domain_claims, domain_verifier),
                )
                .command_handler::<crate::modules::edge::RevokeDomainClaim, _>(
                    RevokeDomainClaimHandler::new(revoke_domain_claims),
                )
                .command_handler::<crate::modules::edge::CreateGatewayScope, _>(
                    CreateGatewayScopeHandler::new(
                        gateway_scope_environments,
                        gateway_scope_nodes,
                        create_gateway_scopes,
                    ),
                )
                .command_handler::<crate::modules::edge::CreateMcpCredential, _>(
                    CreateMcpCredentialHandler::new(
                        mcp_credential_environments,
                        create_mcp_credentials,
                        mcp_credential_issuer,
                        create_mcp_credential_encryption,
                    ),
                )
                .command_handler::<crate::modules::edge::CreateMcpRoutePolicy, _>(
                    CreateMcpRoutePolicyHandler::new(create_mcp_route_policies),
                )
                .command_handler::<crate::modules::edge::ReviseMcpRoutePolicy, _>(
                    ReviseMcpRoutePolicyHandler::new(revise_mcp_route_policies),
                )
                .command_handler::<crate::modules::edge::RotateMcpCredential, _>(
                    RotateMcpCredentialHandler::new(
                        rotate_mcp_credentials,
                        rotate_mcp_credential_issuer,
                        rotate_mcp_credential_encryption,
                    ),
                )
                .command_handler::<crate::modules::edge::RevokeMcpCredential, _>(
                    RevokeMcpCredentialHandler::new(revoke_mcp_credentials),
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
                .command_handler::<crate::modules::fleet::ManageNodePool, _>(
                    ManageNodePoolHandler::new(manage_node_pools),
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
                .command_handler::<crate::modules::plugins::EnrollPluginRegistry, _>(
                    EnrollPluginRegistryHandler::new(
                        plugin_enrollment_authorizer,
                        plugin_trust_roots,
                        Arc::clone(&plugin_registries),
                    ),
                )
                .query_handler::<crate::modules::identity::ListOrganizations, _>(
                    ListOrganizationsHandler::new(query_organizations),
                )
                .query_handler::<crate::modules::identity::ListApiTokens, _>(
                    ListApiTokensHandler::new(list_api_tokens),
                )
                .query_handler::<crate::modules::identity::GetApiToken, _>(
                    GetApiTokenHandler::new(get_api_tokens),
                )
                .query_handler::<crate::modules::identity::ListMemberships, _>(
                    ListMembershipsHandler::new(list_memberships),
                )
                .query_handler::<crate::modules::identity::GetMembership, _>(
                    GetMembershipHandler::new(get_memberships),
                )
                .query_handler::<crate::modules::identity::ListResourceGrants, _>(
                    ListResourceGrantsHandler::new(list_resource_grants),
                )
                .query_handler::<crate::modules::identity::GetResourceGrant, _>(
                    GetResourceGrantHandler::new(get_resource_grants),
                )
                .query_handler::<crate::modules::projects::ListProjects, _>(
                    ListProjectsHandler::new(query_projects),
                )
                .query_handler::<crate::modules::projects::ListEnvironments, _>(
                    ListEnvironmentsHandler::new(list_environment_projects, query_environments),
                )
                .query_handler::<crate::modules::workflow::GetOntology, _>(
                    GetOntologyHandler::new(get_ontologies),
                )
                .query_handler::<crate::modules::workflow::ListOntologies, _>(
                    ListOntologiesHandler::new(list_ontologies),
                )
                .query_handler::<crate::modules::workflow::GetOntologyRevision, _>(
                    GetOntologyRevisionHandler::new(get_ontology_revisions),
                )
                .query_handler::<crate::modules::workflow::ListOntologyRevisions, _>(
                    ListOntologyRevisionsHandler::new(list_ontology_revisions),
                )
                .query_handler::<crate::modules::workflow::DiffOntologyRevisions, _>(
                    DiffOntologyRevisionsHandler::new(diff_ontology_revisions),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowDefinition, _>(
                    GetWorkflowDefinitionHandler::new(get_workflow_definitions),
                )
                .query_handler::<crate::modules::workflow::ListWorkflowDefinitions, _>(
                    ListWorkflowDefinitionsHandler::new(list_workflow_definitions),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowRevision, _>(
                    GetWorkflowRevisionHandler::new(get_workflow_revisions),
                )
                .query_handler::<crate::modules::workflow::ListWorkflowRevisions, _>(
                    ListWorkflowRevisionsHandler::new(list_workflow_revisions),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowGoal, _>(
                    GetWorkflowGoalHandler::new(get_workflow_goals),
                )
                .query_handler::<crate::modules::workflow::ListWorkflowGoals, _>(
                    ListWorkflowGoalsHandler::new(list_workflow_goals),
                )
                .query_handler::<crate::modules::workflow::GetPlanRevision, _>(
                    GetPlanRevisionHandler::new(get_plan_revisions),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowRun, _>(
                    GetWorkflowRunHandler::new(get_workflow_runs),
                )
                .query_handler::<crate::modules::workflow::ListWorkflowRuns, _>(
                    ListWorkflowRunsHandler::new(list_workflow_runs),
                )
                .query_handler::<crate::modules::workflow::WaitWorkflowRun, _>(
                    WaitWorkflowRunHandler::new(wait_workflow_runs),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowRunOutput, _>(
                    GetWorkflowRunOutputHandler::new(get_workflow_run_outputs),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowRunHistory, _>(
                    GetWorkflowRunHistoryHandler::new(
                        get_workflow_run_history_runs,
                        workflow_run_history,
                    ),
                )
                .query_handler::<crate::modules::workflow::GetHumanTask, _>(
                    GetHumanTaskHandler::new(get_human_tasks),
                )
                .query_handler::<crate::modules::workflow::ListHumanTasks, _>(
                    ListHumanTasksHandler::new(list_human_tasks),
                )
                .query_handler::<crate::modules::forms::GetFormDraft, _>(
                    GetFormDraftHandler::new(get_form_drafts),
                )
                .query_handler::<crate::modules::forms::ListFormDrafts, _>(
                    ListFormDraftsHandler::new(list_form_drafts),
                )
                .query_handler::<crate::modules::forms::GetFormRelease, _>(
                    GetFormReleaseHandler::new(get_form_releases),
                )
                .query_handler::<crate::modules::forms::ListFormReleases, _>(
                    ListFormReleasesHandler::new(list_form_releases),
                )
                .query_handler::<crate::modules::search::SearchResources, _>(
                    SearchResourcesHandler::new(search),
                )
                .query_handler::<crate::modules::assets::ListAssets, _>(
                    ListAssetsHandler::new(list_assets),
                )
                .query_handler::<crate::modules::assets::GetAsset, _>(GetAssetHandler::new(
                    get_assets,
                ))
                .query_handler::<crate::modules::assets::ListAssetReleases, _>(
                    ListAssetReleasesHandler::new(list_asset_releases),
                )
                .query_handler::<crate::modules::assets::GetAssetRelease, _>(
                    GetAssetReleaseHandler::new(get_asset_releases),
                )
                .query_handler::<crate::modules::assets::GetMcpServiceProfile, _>(
                    GetMcpServiceProfileHandler::new(get_mcp_service_profiles),
                )
                .query_handler::<crate::modules::assets::SelectAssetRelease, _>(
                    SelectAssetReleaseHandler::new(select_asset_releases),
                )
                .query_handler::<crate::modules::assets::AdvertiseAssetGitRepository, _>(
                    AdvertiseAssetGitRepositoryHandler::new(Arc::clone(&asset_git)),
                )
                .query_handler::<crate::modules::assets::UploadAssetGitPack, _>(
                    UploadAssetGitPackHandler::new(Arc::clone(&asset_git)),
                )
                .query_handler::<crate::modules::assets::AdmitAssetManifest, _>(
                    AdmitAssetManifestHandler::new(asset_git),
                )
                .query_handler::<crate::modules::secrets::ListSecrets, _>(ListSecretsHandler::new(
                    list_secrets,
                ))
                .query_handler::<crate::modules::secrets::GetSecret, _>(GetSecretHandler::new(
                    get_secrets,
                ))
                .query_handler::<crate::modules::sources::ListSourceRevisions, _>(
                    ListSourceRevisionsHandler::new(source_query_environments, list_sources),
                )
                .query_handler::<crate::modules::sources::GetGithubConnection, _>(
                    GetGithubConnectionHandler::new(get_github_connections),
                )
                .query_handler::<crate::modules::sources::ListGithubRepositorySubscriptions, _>(
                    ListGithubRepositorySubscriptionsHandler::new(
                        subscription_query_environments,
                        list_source_subscriptions,
                    ),
                )
                .query_handler::<crate::modules::operations::ListOperations, _>(
                    ListOperationsHandler::new(operations, operation_resource_access),
                )
                .query_handler::<crate::modules::artifacts::ListBuildRuns, _>(
                    ListBuildRunsHandler::new(list_builds),
                )
                .query_handler::<crate::modules::artifacts::GetBuildRun, _>(
                    GetBuildRunHandler::new(get_builds),
                )
                .query_handler::<crate::modules::artifacts::GetBuildEvidence, _>(
                    GetBuildEvidenceHandler::new(get_build_evidence),
                )
                .query_handler::<crate::modules::artifacts::GetBuildRunLogs, _>(
                    GetBuildRunLogsHandler::new(get_build_logs),
                )
                .query_handler::<crate::modules::executions::ListExecutions, _>(
                    ListExecutionsHandler::new(list_executions),
                )
                .query_handler::<crate::modules::executions::GetExecution, _>(
                    GetExecutionHandler::new(get_executions),
                )
                .query_handler::<crate::modules::executions::ListExecutionTemplates, _>(
                    ListExecutionTemplatesHandler::new(
                        list_execution_template_projects,
                        list_execution_templates,
                    ),
                )
                .query_handler::<crate::modules::executions::GetExecutionTemplate, _>(
                    GetExecutionTemplateHandler::new(get_execution_templates),
                )
                .query_handler::<crate::modules::agents::ListAgentConversations, _>(
                    ListAgentConversationsHandler::new(list_agent_conversations),
                )
                .query_handler::<crate::modules::agents::GetAgentConversation, _>(
                    GetAgentConversationHandler::new(get_agent_conversations),
                )
                .query_handler::<crate::modules::agents::ListAgentExecutions, _>(
                    ListAgentExecutionsHandler::new(list_agent_executions),
                )
                .query_handler::<crate::modules::agents::GetAgentExecution, _>(
                    GetAgentExecutionHandler::new(get_agent_executions),
                )
                .query_handler::<crate::modules::agents::GetAgentExecutionChangeSet, _>(
                    GetAgentExecutionChangeSetHandler::new(get_agent_execution_change_sets),
                )
                .query_handler::<crate::modules::agents::GetAgentExecutionEvents, _>(
                    GetAgentExecutionEventsHandler::new(get_agent_execution_events),
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
                .query_handler::<crate::modules::fleet::GetNodePool, _>(
                    GetNodePoolHandler::new(get_node_pools),
                )
                .query_handler::<crate::modules::fleet::ListNodePools, _>(
                    ListNodePoolsHandler::new(list_node_pools),
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
                .query_handler::<crate::modules::edge::ListGatewayScopes, _>(
                    ListGatewayScopesHandler::new(list_gateway_scopes),
                )
                .query_handler::<crate::modules::edge::ListMcpCredentials, _>(
                    ListMcpCredentialsHandler::new(list_mcp_credentials),
                )
                .query_handler::<crate::modules::edge::GetMcpCredential, _>(
                    GetMcpCredentialHandler::new(get_mcp_credentials),
                )
                .query_handler::<crate::modules::edge::ListMcpRoutePolicies, _>(
                    ListMcpRoutePoliciesHandler::new(list_mcp_route_policies),
                )
                .query_handler::<crate::modules::edge::GetMcpRoutePolicy, _>(
                    GetMcpRoutePolicyHandler::new(get_mcp_route_policies),
                )
                .query_handler::<crate::modules::edge::GetRoute, _>(GetRouteHandler::new(
                    get_routes,
                ))
                .query_handler::<crate::modules::plugins::ListPluginRegistries, _>(
                    ListPluginRegistriesHandler::new(Arc::clone(&plugin_registries)),
                )
                .query_handler::<crate::modules::plugins::GetPluginRegistry, _>(
                    GetPluginRegistryHandler::new(Arc::clone(&plugin_registries)),
                )
                .query_handler::<crate::modules::plugins::SearchPluginCatalog, _>(
                    SearchPluginCatalogHandler::new(
                        Arc::clone(&plugin_registries),
                        Arc::clone(&plugin_catalog),
                    ),
                )
                .query_handler::<crate::modules::plugins::SearchCachedPluginCatalog, _>(
                    SearchCachedPluginCatalogHandler::new(
                        Arc::clone(&plugin_registries),
                        Arc::clone(&plugin_catalog),
                    ),
                )
                .query_handler::<crate::modules::plugins::InspectPluginCatalog, _>(
                    InspectPluginCatalogHandler::new(
                        Arc::clone(&plugin_registries),
                        Arc::clone(&plugin_catalog),
                    ),
                )
                .query_handler::<crate::modules::plugins::InspectCachedPluginCatalog, _>(
                    InspectCachedPluginCatalogHandler::new(plugin_registries, plugin_catalog),
                )
                .global(),
        )
        .import(IdentityModule::new(bootstrap_credential))
        .import(ProjectsModule)
        .import(WorkflowModule)
        .import(FormsModule)
        .import(SearchModule)
        .import(SecretsModule)
        .import(SourcesModule::new(source_webhook_verifier))
        .import(AssetsModule::new(config.assets.max_rpc_body_bytes)?)
        .import(ArtifactsModule)
        .import(ExecutionsModule)
        .import(AgentsModule)
        .import(OperationsModule)
        .import(PluginsModule)
        .import(FleetModule::new(heartbeat_timeout)?)
        .import(WorkloadsModule)
        .import(EdgeModule)
        .import(PlatformModule::new(&config))
        .import(ManagementMcpModule)
        .import(ApiContractModule)
        .use_global_middleware(RequestIdMiddleware)
        .use_global_auth()
        .use_global_interceptor(ApiResponseInterceptor)
        .use_global_filter(ApiErrorFilter)
        .global_prefix(API_PREFIX)
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

async fn build_evidence_signer(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn IBuildEvidenceSigner>, ControlPlaneStartupError> {
    match config.security.build_evidence_signing {
        SecurityProviderKind::Local => Ok(Arc::new(
            LocalBuildEvidenceSigner::load_or_create(
                std::path::Path::new(&config.security.state_dir)
                    .join("build-evidence/signing-key.pk8"),
            )
            .await
            .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
        )),
        SecurityProviderKind::Vault => {
            let (address, token) = credentials.ok_or_else(|| {
                ControlPlaneStartupError::Security("Vault credentials were not resolved".into())
            })?;
            Ok(Arc::new(
                VaultBuildEvidenceSigner::new(
                    address,
                    token,
                    config.security.vault_transit_mount.clone(),
                    config.security.vault_build_evidence_signing_key.clone(),
                    Duration::from_millis(config.security.vault_timeout_ms),
                )
                .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
            ))
        }
    }
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
            LogChunkObjectStore::local(&config.security.state_dir)
                .map_err(|error| ControlPlaneStartupError::LogStorage(error.to_string()))?,
        )),
        LogStorageProviderKind::S3 => {
            let credentials = config.s3_log_credentials()?.ok_or_else(|| {
                ControlPlaneStartupError::LogStorage("S3 credentials were not resolved".into())
            })?;
            let objects = ImmutableObjectClient::s3(S3ImmutableObjectOptions {
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
            .map_err(|error| ControlPlaneStartupError::LogStorage(error.to_string()))?;
            Ok(Arc::new(LogChunkObjectStore::from_client(objects)))
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
