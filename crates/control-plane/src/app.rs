use crate::infrastructure::{
    ImmutableObjectClient, OperationResourceAccessResolver, S3ImmutableObjectOptions,
    SmtpCredentials, SmtpTlsPolicy, SmtpTransport, SmtpTransportOptions,
};
use crate::modules::agents::{
    AgentExecutionFlowRuntime, AgentExecutionFlowRuntimeDependencies, AgentExecutionReconciler,
    AgentsModule, AppendAgentExecutionEventsHandler, CancelAgentExecutionHandler,
    CreateAgentConversationHandler, GetAgentConversationHandler, GetAgentExecutionChangeSetHandler,
    GetAgentExecutionEventsHandler, GetAgentExecutionHandler, IAgentRepository,
    ListAgentConversationsHandler, ListAgentExecutionsHandler, StartAgentExecutionHandler,
};
use crate::modules::applications::{
    AdmitApplicationInvocationHandler, AdmitApplicationSessionHandler, ApplicationsModule,
    CancelApplicationInvocationHandler, CloseApplicationSessionHandler,
    CompileApplicationPresetWorkflowHandler, ComposeApplicationInvocationWorkflowRunHandler,
    CreateApplicationHandler, GetApplicationHandler, GetApplicationInvocationHandler,
    GetApplicationReleaseHandler, GetApplicationSessionHandler, IApplicationOntologyRevisionPort,
    IApplicationPresetWorkflowPort, IApplicationRepository, IApplicationSessionRepository,
    IApplicationWorkflowRevisionPort, IApplicationWorkflowRunPort, IWorkflowApplicationEffectsPort,
    ListApplicationReleasesHandler, ListApplicationsHandler, OpenApplicationSessionHandler,
    PublishApplicationReleaseHandler, ReplayApplicationSessionHandler,
    RequestApplicationInvocationHandler, WorkflowApplicationEffectsService,
    WorkflowApplicationOntologyRevisionReader, WorkflowApplicationPresetCompiler,
    WorkflowApplicationReleaseEvidenceReader, WorkflowApplicationRunService,
};
use crate::modules::artifacts::application::BuildRunReconciler;
use crate::modules::artifacts::{
    ArtifactsModule, BoxBuildEvidenceGenerator, BuildCandidateProjector, BuildFlowRuntime,
    BuildFlowRuntimeDependencies, CancelBuildRunHandler, CloudBuildSourceResolver,
    GetBuildEvidenceHandler, GetBuildRunHandler, GetBuildRunLogsHandler,
    HostedArtifactQueryService, IBuildArtifactPublisher, IBuildCandidateProjectionPort,
    IBuildEvidenceGenerator, IBuildEvidenceSigner, IBuildInputPreparer, IBuildOutputValidator,
    IBuildRunRepository, IBuildSourceResolver, IHostedArtifactQueryPort, INodeArtifactStore,
    ListBuildRunsHandler, LocalBuildEvidenceSigner, NodeArtifactObjectStore,
    OciBuildOutputValidator, OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions,
    RetryBuildRunHandler, SourceBuildInputPreparer, VaultBuildEvidenceSigner,
};
use crate::modules::assets::{
    AdmitAssetManifestHandler, AdvertiseAssetGitRepositoryHandler, ArchiveAssetHandler,
    AssetCatalogApplicationService, AssetGitApplicationService, AssetGitApplicationServiceOptions,
    AssetsModule, BackupAssetGitRepositoryHandler, BindMcpServiceProfileHandler,
    CreateAssetHandler, CreateAssetReleaseHandler, GetAssetHandler, GetAssetReleaseHandler,
    GetMcpServiceProfileHandler, HostedAssetBuildInputQueryService, HostedBuildOutcomeProjector,
    IAssetGitRepository, IAssetRepository, IHostedAssetBuildInputQueryPort,
    ListAssetReleasesHandler, ListAssetsHandler, LocalAssetGitRepository,
    McpServiceProfileApplicationService, ReceiveAssetGitPackHandler,
    RestoreAssetGitRepositoryHandler, SelectAssetReleaseHandler, UploadAssetGitPackHandler,
    YankAssetReleaseHandler,
};
use crate::modules::audit::{
    AuditExportSigningError, AuditExportSigningKey, AuditModule, AuditRetentionPolicy,
    AuditRetentionWorker, ExportAuditManifestHandler, ExportAuditRecordsHandler,
    GetAuditRetentionStatusHandler, IAuditExportSigner, IAuditRecordRepository,
    ListAuditRecordsHandler, VerifiedAuditExportSignature,
};
use crate::modules::connectors::{
    ConnectorExecutionApplicationService, ConnectorExecutionServiceOptions,
    ConnectorHttpExecutionPreparationPort, ConnectorHttpRevisionMaterializer,
    ConnectorResponseObjectStore, ConnectorsModule, CreateConnectorProfileHandler,
    GetConnectorExecutionAttemptHandler, GetConnectorExecutionAttemptResolutionHandler,
    GetConnectorProfileHandler, GetConnectorRevisionHandler, GetConnectorRevisionRevocationHandler,
    IConnectorExecutionAttemptRepository, IConnectorExecutionAttemptResolutionRepository,
    IConnectorProfileRepository, IConnectorResponseObjectPort,
    IConnectorRevisionRevocationRepository, ListConnectorProfilesHandler,
    ListConnectorRevisionsHandler, ListUnresolvedConnectorExecutionAttemptsHandler,
    PublicInternetConnectorEgressAuthorizer, ResolveConnectorExecutionAttemptHandler,
    ReviseConnectorProfileHandler, RevokeConnectorRevisionHandler,
    WorkflowConnectorApplicationService,
};
use crate::modules::data::{
    ObjectNamespaceCredentialMaterializer, ObjectNamespaceRecoveryFlowRuntime,
};
use crate::modules::developer_workflows::{
    IPreviewEnvironmentPort, IPullRequestPreviewPolicyRepository,
    IPullRequestPreviewProjectionPort, IPullRequestPreviewProjectionRepository,
    ProjectsPreviewEnvironmentAdapter, PullRequestPreviewProjectionService,
    PullRequestPreviewProjector,
};
use crate::modules::durable_cells::{
    CreateDurableCellApplicationHandler, DeployDurableCellApplicationFromAclHandler,
    DeployDurableCellApplicationHandler, DurableCellBundlePublicationGate,
    DurableCellPriorWriterSeal, DurableCellWriterFenceAdapter, DurableCellsModule,
    GetDurableCellApplicationHandler, GetDurableCellApplicationRevisionHandler,
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    ListDurableCellApplicationRevisionsHandler, ListDurableCellApplicationsHandler,
    PublishDurableCellApplicationRouteHandler, ReviseDurableCellApplicationHandler,
    StartDurableCellApplicationHandler, StopDurableCellApplicationHandler,
};
use crate::modules::edge::domain::repositories::{
    IEdgeRepository, IMcpCredentialLifecycleRepository,
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
    PublishRouteHandler, ReviseMcpRoutePolicyHandler, RevokeDomainClaimHandler,
    RevokeMcpCredentialHandler, RotateMcpCredentialHandler, VaultGatewayCertificateAuthority,
    VerifyDomainClaimHandler, WorkloadRouteTargetReader,
};
use crate::modules::executions::{
    CancelExecutionHandler, CreateExecutionHandler, CreateExecutionTemplateHandler,
    ExecutionFlowRuntime, ExecutionFlowRuntimeDependencies, ExecutionReconciler, ExecutionsModule,
    GetExecutionHandler, GetExecutionTemplateHandler, IExecutionRepository,
    IExecutionTemplateRepository, IWorkflowExecutionPort, ListExecutionTemplatesHandler,
    ListExecutionsHandler, WorkflowExecutionApplicationService,
};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodePoolRepository, INodeRepository,
};
use crate::modules::fleet::domain::services::{ICertificateAuthority, ILogChunkStore};
use crate::modules::fleet::{
    AcknowledgeNodeCommandHandler, ChangeNodeStateHandler, EnqueueNodeCommandHandler,
    EnrollNodeHandler, FleetModule, GetNodeHandler, GetNodePoolHandler,
    IGatewayAcknowledgementProjector, IssueEnrollmentTokenHandler, LeaseNodeCommandsHandler,
    ListNodePoolsHandler, ListNodesHandler, LocalCertificateAuthority, LocalKeyEncryptionService,
    LogChunkObjectStore, LogCompactionWorker, LogRetentionWorker, ManageNodePoolHandler,
    NodeAvailabilityReconciler, NodeControlApi, NodeControlServer,
    RecordGatewayAcknowledgementHandler, RecordNodeLogChunksHandler, RecordNodeObservationsHandler,
    RotateNodeCertificateHandler, VaultCertificateAuthority, VaultKeyEncryptionService,
};
use crate::modules::forms::{
    CreateFormDraftHandler, FormsModule, GetFormDraftHandler, GetFormReleaseHandler,
    IFormRepository, IFormSemanticCore, ListFormDraftsHandler, ListFormReleasesHandler,
    NativeFormSemanticCore, PublishFormReleaseHandler, ReviseFormDraftHandler,
};
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IMembershipInvitationRepository, IMembershipRepository,
    IOidcIdentityRepository, IOrganizationRepository, IRecipientContactRepository,
    IResourceAuthorizationDecisionRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::services::{
    IOidcProviderService, IRecipientContactProofService,
};
use crate::modules::identity::domain::value_objects::{
    BootstrapCredential, RecipientContactSigningKeyId,
};
use crate::modules::identity::infrastructure::{
    ApiTokenVerifier, HmacRecipientContactProofService, VaultRecipientContactProofService,
};
use crate::modules::identity::{
    A3sEventRecipientContactVerificationConsumer, AcceptMembershipInvitationHandler,
    BeginOidcFlowHandler, BeginRecipientContactVerificationHandler, BootstrapIdentityHandler,
    ChangeMembershipRoleHandler, CompleteOidcFlowHandler,
    CompleteRecipientContactVerificationHandler, CreateApiTokenHandler, CreateMembershipHandler,
    CreateMembershipInvitationHandler, CreateOrganizationHandler, CreateResourceGrantHandler,
    GetApiTokenHandler, GetMembershipHandler, GetMembershipInvitationHandler,
    GetRecipientContactHandler, GetResourceGrantHandler, IdentityModule, ListApiTokensHandler,
    ListMembershipInvitationsHandler, ListMembershipsHandler, ListMyMembershipInvitationsHandler,
    ListOrganizationsHandler, ListRecipientContactsHandler, ListResourceGrantsHandler,
    OpenIdConnectProviderService, RecipientContactVerificationDeliveryDispatcher,
    RevokeApiTokenHandler, RevokeMembershipHandler, RevokeMembershipInvitationHandler,
    RevokeRecipientContactHandler, RevokeResourceGrantHandler,
    SmtpRecipientContactVerificationDeliveryService,
    RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY,
};
use crate::modules::integration_events::{
    A3sEventPublisher, EventPublishError, IEventPublisher, IIntegrationEventProjector,
    IOutboxRepository, OutboxRelay, OutboxRelayConfig,
};
use crate::modules::notifications::infrastructure::SmtpOutboundNotificationDeliveryService;
use crate::modules::notifications::{
    A3sEventOutboundNotificationConsumer, CreateNotificationAlertPolicyHandler,
    CreateOutboundNotificationSubscriptionHandler, GetNotificationAlertPolicyHandler,
    GetNotificationHandler, GetOutboundNotificationSubscriptionHandler,
    INotificationAlertPolicyRepository, INotificationRepository, IOutboundNotificationDispatcher,
    IOutboundNotificationRepository, ListNotificationAlertPoliciesHandler,
    ListNotificationsHandler, ListOutboundNotificationSubscriptionsHandler,
    MarkNotificationReadHandler, NotificationsModule, OutboundNotificationDispatcher,
    OutboundNotificationSmtpDispatcher, OutboxNotificationProjector,
    RevokeNotificationAlertPolicyHandler, RevokeOutboundNotificationSubscriptionHandler,
    OUTBOUND_NOTIFICATION_EVENT_KEY,
};
use crate::modules::operations::{
    FlowOperationEngine, IOperationRepository, ListOperationsHandler, OperationReconciler,
    OperationsModule, ReconcileOperationsHandler,
};
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginRegistryEnrollmentAuthorizer, IPluginTrustRootStore,
};
use crate::modules::plugins::{
    A3sUsePluginRegistryCatalog, EnrollPluginRegistryHandler, GetPluginRegistryHandler,
    InspectCachedPluginCatalogHandler, InspectPluginCatalogHandler, ListPluginRegistriesHandler,
    PluginTrustRootObjectStore, PluginsModule, SearchCachedPluginCatalogHandler,
    SearchPluginCatalogHandler,
};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::{
    CreateEnvironmentHandler, CreateProjectHandler, GetProjectAttributionHandler,
    ListEnvironmentsHandler, ListProjectsHandler, ProjectsModule, UpdateProjectAttributionHandler,
};
use crate::modules::search::{ISearchRepository, SearchModule, SearchResourcesHandler};
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::secrets::{
    CreateSecretHandler, GetSecretHandler, ListSecretsHandler, RevokeSecretVersionHandler,
    RotateSecretHandler, SecretsModule,
};
use crate::modules::security::{
    IGatewayRoutePolicyTimelineRepository, ListGatewayRoutePolicyTimelineHandler, SecurityModule,
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
    DeactivateGithubRepositorySubscriptionHandler, ExternalSourceBuildArchiveAdapter,
    GetGithubConnectionHandler, GitSourceCheckout, GithubAppClient,
    GithubConnectionAuthorityReconciler, GithubInstallationTokenIssuer, GithubSourceResolver,
    GithubWebhookVerifier, ISourceBuildInputQueryPort, ListGithubRepositorySubscriptionsHandler,
    ListSourceRevisionsHandler, PrepareGithubConnectionOauthHandler,
    ReconcileGithubConnectionLifecycleHandler, ResolveExternalSourceRevisionHandler,
    RevalidatingGithubInstallationTokens, SourceBuildInputQueryService, SourcesModule,
};
use crate::modules::workflow::{
    CancelWorkflowRunHandler, ChangeHumanTaskAssignmentHandler, CreateOntologyHandler,
    CreateWorkflowDefinitionHandler, CreateWorkflowGoalHandler, DiffOntologyRevisionsHandler,
    FlowWorkflowRunCoordinator, GetHumanTaskHandler, GetOntologyHandler,
    GetOntologyRevisionHandler, GetPlanRevisionHandler, GetWorkflowDefinitionHandler,
    GetWorkflowGoalHandler, GetWorkflowNodeCatalogHandler, GetWorkflowRevisionHandler,
    GetWorkflowRunDiagnosticsHandler, GetWorkflowRunHandler, GetWorkflowRunHistoryHandler,
    GetWorkflowRunOutputHandler, GetWorkflowRunVariablesHandler, HumanTaskCoordinator,
    HumanTaskResumeWorker, HumanTaskResumeWorkerConfig, IHumanTaskRepository, IOntologyRepository,
    IWorkflowCompositeExecutionPort, IWorkflowDefinitionPublicationPort,
    IWorkflowDefinitionRepository, IWorkflowGoalRepository, IWorkflowRunCoordinator,
    IWorkflowRunDiagnosticsReader, IWorkflowRunHistoryReader, IWorkflowRunRepository,
    IWorkflowRunVariableReader, ListHumanTasksHandler, ListOntologiesHandler,
    ListOntologyRevisionsHandler, ListWorkflowDefinitionsHandler, ListWorkflowGoalsHandler,
    ListWorkflowRevisionsHandler, ListWorkflowRunsHandler, ReviseOntologyHandler,
    ReviseWorkflowDefinitionHandler, StartWorkflowRunHandler, SubmitHumanTaskHandler,
    WaitWorkflowRunHandler, WorkflowCompositeExecutionApplicationService,
    WorkflowDefinitionPublicationService, WorkflowModule, WorkflowRunDiagnosticsReader,
    WorkflowRunFlowRuntime, WorkflowRunHistoryReader, WorkflowRunReconciler,
    WorkflowRunVariableReader,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::domain::services::{
    IDeploymentRouteUpdater, IOciArtifactResolver, IWorkloadPrestartGate,
};
use crate::modules::workloads::{
    BindSkillWorkloadDeploymentHandler, CancelDeploymentHandler,
    CreateAgentWorkloadDeploymentHandler, CreateSourceWorkloadDeploymentHandler,
    CreateWorkloadDeploymentHandler, DeploymentFlowConfig, DeploymentFlowDependencies,
    DeploymentFlowRuntime, GetDeploymentHandler, GetWorkloadHandler, GetWorkloadLogsHandler,
    ListWorkloadsHandler, NodeDrainEvacuationReconciler, OciRegistryArtifactResolver,
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
        EventProviderKind, ObjectStorageProviderKind, ProcessRole, SecurityProfile,
        SecurityProviderKind, SmtpProviderKind, SmtpTlsMode,
    },
    infrastructure::{
        bind_infrastructure, connect_postgres, postgres_health, FlowReadInfrastructure,
        FlowRuntimeRouter, InfrastructureBinding, PostgresBootstrapError,
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
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

mod postgres_adapters;

use postgres_adapters::{ApiWorkerPostgresAdapters, PostgresAdapterFactory, RelayPostgresAdapters};

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
    #[error("could not initialize Connector execution: {0}")]
    Connector(String),
    #[error("could not initialize outbound notification delivery: {0}")]
    Notification(String),
    #[error("could not initialize recipient contact verification delivery: {0}")]
    Smtp(String),
    #[error("could not initialize security providers: {0}")]
    Security(String),
    #[error("could not initialize Edge providers: {0}")]
    Edge(String),
    #[error("could not initialize log retention or compaction: {0}")]
    LogMaintenance(String),
    #[error("could not initialize audit retention: {0}")]
    AuditMaintenance(String),
    #[error("could not initialize shared object storage: {0}")]
    ObjectStorage(String),
    #[error("could not bind deployment infrastructure: {0}")]
    InfrastructureBinding(String),
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
    #[error("could not initialize object namespace recovery: {0}")]
    ObjectNamespaceRecovery(String),
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
    build_application_with_overrides(config, None, None).await
}

#[doc(hidden)]
pub async fn build_application_with_source_resolver(
    config: CloudConfig,
    source_resolver: Arc<dyn ISourceResolver>,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    build_application_with_overrides(config, Some(source_resolver), None).await
}

#[doc(hidden)]
pub async fn build_application_with_source_resolver_and_oidc_provider(
    config: CloudConfig,
    source_resolver: Arc<dyn ISourceResolver>,
    oidc_provider: Arc<dyn IOidcProviderService>,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    build_application_with_overrides(config, Some(source_resolver), Some(oidc_provider)).await
}

async fn build_application_with_overrides(
    config: CloudConfig,
    source_resolver: Option<Arc<dyn ISourceResolver>>,
    oidc_provider: Option<Arc<dyn IOidcProviderService>>,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    config.validate()?;
    if config.server.role == ProcessRole::Relay {
        return build_relay_application(config).await;
    }

    let management_adapters = if config.server.role.serves_management_api() {
        let source_resolver = match source_resolver {
            Some(source_resolver) => source_resolver,
            None => Arc::new(
                GithubSourceResolver::new(Duration::from_millis(
                    config.sources.github_request_timeout_ms,
                ))
                .map_err(ControlPlaneStartupError::Sources)?,
            ),
        };
        let oidc_provider = match oidc_provider {
            Some(oidc_provider) => oidc_provider,
            None => Arc::new(
                OpenIdConnectProviderService::new(&config.auth.oidc_providers)
                    .map_err(ControlPlaneStartupError::Auth)?,
            ),
        };
        Some(ManagementAdapterOverrides {
            source_resolver,
            oidc_provider,
        })
    } else {
        None
    };
    build_api_worker_application(config, management_adapters).await
}

struct ManagementAdapterOverrides {
    source_resolver: Arc<dyn ISourceResolver>,
    oidc_provider: Arc<dyn IOidcProviderService>,
}

async fn build_api_worker_application(
    config: CloudConfig,
    management_adapters: Option<ManagementAdapterOverrides>,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let run_operations = config.server.role.runs_workers();
    let run_relay = config.server.role.runs_relay();
    let serving_postgres_url = config.serving_postgres_url()?;
    let executor = connect_postgres(&serving_postgres_url, config.postgres.max_connections).await?;
    let object_storage = object_storage(&config)?;
    if !object_storage
        .health()
        .await
        .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))?
    {
        return Err(ControlPlaneStartupError::ObjectStorage(
            "shared object storage startup probe returned unhealthy".into(),
        ));
    }
    bind_infrastructure(
        &executor,
        InfrastructureBinding::new(
            "object-storage",
            "a3s.cloud.object-storage-topology.v1",
            object_storage.infrastructure_identity(),
        )
        .map_err(|error| ControlPlaneStartupError::InfrastructureBinding(error.to_string()))?,
    )
    .await
    .map_err(|error| ControlPlaneStartupError::InfrastructureBinding(error.to_string()))?;
    let asset_backup_objects = object_storage
        .subnamespace("asset-git-backups")
        .map_err(|error| ControlPlaneStartupError::Assets(error.to_string()))?;
    let asset_git_repository = LocalAssetGitRepository::new(
        &config.assets.repository_dir,
        Duration::from_millis(config.assets.git_command_timeout_ms),
    )
    .and_then(|repository| {
        repository.with_backup_objects(asset_backup_objects, config.assets.backup_max_bytes)
    })
    .map_err(|error| ControlPlaneStartupError::Assets(error.to_string()))?;
    bind_infrastructure(
        &executor,
        InfrastructureBinding::new(
            "asset-git-storage",
            "a3s.cloud.asset-git-storage.v1",
            asset_git_repository.infrastructure_identity(),
        )
        .map_err(|error| ControlPlaneStartupError::InfrastructureBinding(error.to_string()))?,
    )
    .await
    .map_err(|error| ControlPlaneStartupError::InfrastructureBinding(error.to_string()))?;
    let asset_git_repositories: Arc<dyn IAssetGitRepository> = Arc::new(asset_git_repository);
    let event_publisher = if config.server.role.owns_event_transport() {
        Some(event_publisher(&config).await?)
    } else {
        None
    };
    let vault_credentials = config.vault_credentials()?;
    let audit_export_signer = if config.server.role.serves_management_api() {
        Some(audit_export_signer(&config, vault_credentials.as_ref()).await?)
    } else {
        None
    };
    let key_encryption = key_encryption_provider(&config, vault_credentials.as_ref())?;
    let recipient_contact_proof =
        recipient_contact_proof_provider(&config, vault_credentials.as_ref())?;
    let gateway_certificate_authority =
        gateway_certificate_authority(&config, vault_credentials.as_ref())?;
    let log_chunks: Arc<dyn ILogChunkStore> = Arc::new(LogChunkObjectStore::from_client(
        object_storage
            .subnamespace("logs")
            .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))?,
    ));
    let postgres_adapters = PostgresAdapterFactory::new(executor.clone());
    let developer_workflow_projection =
        run_relay.then(|| postgres_adapters.developer_workflow_projection());
    let adapters: ApiWorkerPostgresAdapters = postgres_adapters.api_worker();
    let organizations = adapters.identity.organizations;
    let api_tokens = adapters.identity.api_tokens;
    let memberships = adapters.identity.memberships;
    let membership_invitations = adapters.identity.membership_invitations;
    let resource_grants = adapters.identity.resource_grants;
    let oidc_identity = adapters.identity.oidc_identity;
    let recipient_contacts = adapters.identity.recipient_contacts;
    let recipient_contact_verification_deliveries =
        adapters.identity.recipient_contact_verification_deliveries;
    let resource_authorization_decisions = adapters.identity.resource_authorization_decisions;
    let projects = adapters.projects.projects;
    let environments = adapters.projects.environments;
    let ontologies = adapters.workflow.ontologies;
    let workflow_definitions = adapters.workflow.workflow_definitions;
    let workflow_goals = adapters.workflow.workflow_goals;
    let workflow_runs = adapters.workflow.workflow_runs;
    let forms = adapters.workflow.forms;
    let human_tasks = adapters.workflow.human_tasks;
    let form_semantic_core: Arc<dyn IFormSemanticCore> = Arc::new(NativeFormSemanticCore::new());
    let search = adapters.search;
    let audit_records = adapters.audit_records;
    let audit_retention_repository = Arc::clone(&audit_records);
    let security_investigations = adapters.security_investigations;
    let notifications = adapters.notifications.notifications;
    let alert_policies = adapters.notifications.alert_policies;
    let outbound_notifications = adapters.notifications.outbound_notifications;
    let outbound_notification_deliveries = adapters.notifications.outbound_deliveries;
    let outbound_notification_smtp_attempts = adapters.notifications.outbound_smtp_attempts;
    let plugin_registries = adapters.plugins.registries;
    let plugin_enrollment_authorizer = adapters.plugins.enrollment_authorizer;
    let nodes = adapters.fleet.nodes;
    let node_availability = adapters.fleet.node_availability;
    let scheduling_nodes = adapters.fleet.scheduling_nodes;
    let node_pools = adapters.fleet.node_pools;
    let draining_nodes = adapters.fleet.draining_nodes;
    let node_control = adapters.fleet.node_control;
    let node_artifacts: Arc<dyn INodeArtifactStore> = Arc::new(
        NodeArtifactObjectStore::from_client(
            object_storage
                .subnamespace("artifacts")
                .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))?,
            config.artifacts.max_blob_bytes,
        )
        .map_err(ControlPlaneStartupError::ObjectStorage)?,
    );
    let builds = adapters.builds;
    let build_candidates = adapters.build_candidates;
    let executions = adapters.executions;
    let execution_templates = adapters.execution_templates;
    let agents = adapters.agents;
    let log_retention_repository = adapters.fleet.log_retention;
    let workload_runtime_control = adapters.fleet.workload_runtime_control;
    let workloads = adapters.workloads.workloads;
    let deployment_workloads = adapters.workloads.deployment_workloads;
    let replica_deployments = adapters.workloads.replica_deployments;
    let replica_evacuations = adapters.workloads.replica_evacuations;
    let replica_retirements = adapters.workloads.replica_retirements;
    let writer_fences = adapters.workloads.writer_fences;
    let operation_repository = adapters.operations;
    let workload_targets = adapters.workloads.workload_targets;
    let secret_rotation_restarts = adapters.workloads.secret_rotation_restarts;
    let resource_claims = adapters.workloads.resource_claims;
    let routes = adapters.edge.routes;
    let mcp_credentials = adapters.edge.mcp_credentials;
    let mcp_credential_reader = adapters.edge.mcp_credential_reader;
    let mcp_route_policy_repository = adapters.edge.mcp_route_policies;
    let mcp_gateway_snapshots = adapters.edge.mcp_gateway_snapshots;
    let assets = adapters.assets.assets;
    let asset_controls = adapters.assets.controls;
    let mcp_profiles = adapters.assets.mcp_profiles;
    let secrets = adapters.secrets;
    let connector_profiles = adapters.connector_profiles;
    let connector_execution_adapters = postgres_adapters.connector_execution();
    let connector_attempts = connector_execution_adapters.attempts;
    let connector_attempt_resolutions = connector_execution_adapters.resolutions;
    let connector_revocations = connector_execution_adapters.revocations;
    let applications = adapters.applications;
    let application_sessions = adapters.application_sessions;
    let durable_cell_applications = adapters.durable_cell_applications;
    let durable_cell_deployments = adapters.durable_cell_deployments;
    let connector_execution = if run_operations {
        let connector_response_objects = Arc::new(ConnectorResponseObjectStore::from_client(
            object_storage
                .subnamespace("connector-responses")
                .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))?,
        ));
        let connector_materializer = ConnectorHttpRevisionMaterializer::new(
            Arc::clone(&secrets),
            Arc::clone(&key_encryption),
        );
        let connector_egress = Arc::new(
            PublicInternetConnectorEgressAuthorizer::from_system_config(Duration::from_secs(5))
                .map_err(ControlPlaneStartupError::Connector)?,
        );
        let connector_preparation = Arc::new(ConnectorHttpExecutionPreparationPort::new(
            connector_materializer,
            connector_egress,
        ));
        Some(Arc::new(
            ConnectorExecutionApplicationService::new(
                Arc::clone(&connector_profiles),
                Arc::clone(&connector_attempts),
                connector_preparation,
                ConnectorExecutionServiceOptions::default(),
            )
            .map_err(ControlPlaneStartupError::Connector)?
            .with_response_object_store(connector_response_objects),
        ))
    } else {
        None
    };
    let smtp_delivery_transport = if run_operations
        && config.events.provider == EventProviderKind::Nats
        && config.smtp.provider == SmtpProviderKind::Relay
    {
        let credentials = config.smtp_credentials()?.ok_or_else(|| {
            ControlPlaneStartupError::Smtp("SMTP credentials were not resolved".into())
        })?;
        let sender = crate::modules::identity::domain::value_objects::RecipientEmailAddress::parse(
            &config.smtp.sender,
        )
        .map_err(ControlPlaneStartupError::Smtp)?;
        let tls_policy = match config.smtp.tls {
            SmtpTlsMode::RequiredStartTls => SmtpTlsPolicy::RequiredStartTls,
            SmtpTlsMode::Implicit => SmtpTlsPolicy::Implicit,
        };
        let transport = SmtpTransport::new(SmtpTransportOptions {
            host: config.smtp.host.clone(),
            port: config.smtp.port,
            tls_policy,
            hello_name: config.smtp.hello_name.clone(),
            ca_certificate_file: config.smtp.ca_certificate_file.clone(),
            credentials: SmtpCredentials {
                username: credentials.username,
                password: credentials.password,
            },
            connect_timeout: Duration::from_millis(config.smtp.connect_timeout_ms),
            command_timeout: Duration::from_millis(config.smtp.command_timeout_ms),
        })
        .map_err(ControlPlaneStartupError::Smtp)?;
        Some((sender, Arc::new(transport)))
    } else {
        None
    };
    let outbound_notification_consumer = if run_operations
        && config.events.provider == EventProviderKind::Nats
    {
        let event_publisher = event_publisher.as_ref().ok_or_else(|| {
            ControlPlaneStartupError::Framework(BootError::Internal(
                "worker process is missing its event publisher".into(),
            ))
        })?;
        let connector_execution = connector_execution.as_ref().ok_or_else(|| {
            ControlPlaneStartupError::Connector(
                "worker process is missing Connector execution".into(),
            )
        })?;
        let mut dispatcher = OutboundNotificationDispatcher::new(Arc::clone(connector_execution));
        if let Some((sender, transport)) = &smtp_delivery_transport {
            let smtp_delivery_service = Arc::new(SmtpOutboundNotificationDeliveryService::new(
                sender.clone(),
                Arc::clone(transport),
            ));
            let smtp_dispatcher = OutboundNotificationSmtpDispatcher::new(
                Arc::clone(&outbound_notification_smtp_attempts),
                Arc::clone(&recipient_contacts),
                smtp_delivery_service,
                chrono_duration(config.smtp.reservation_lease_ms)?,
                chrono_duration(config.smtp.command_timeout_ms)?,
            )
            .map_err(ControlPlaneStartupError::Notification)?;
            dispatcher = dispatcher.with_smtp_dispatcher(Arc::new(smtp_dispatcher));
        }
        let dispatcher: Arc<dyn IOutboundNotificationDispatcher> = Arc::new(dispatcher);
        Some(
            A3sEventOutboundNotificationConsumer::new(
                event_publisher.bus(),
                event_publisher.subject(OUTBOUND_NOTIFICATION_EVENT_KEY),
                outbound_notification_deliveries,
                dispatcher,
            )
            .map_err(ControlPlaneStartupError::Notification)?,
        )
    } else {
        None
    };
    let recipient_contact_verification_consumer = if run_operations
        && config.events.provider == EventProviderKind::Nats
        && config.smtp.provider == SmtpProviderKind::Relay
    {
        let event_publisher = event_publisher.as_ref().ok_or_else(|| {
            ControlPlaneStartupError::Framework(BootError::Internal(
                "worker process is missing its event publisher".into(),
            ))
        })?;
        let (sender, transport) = smtp_delivery_transport.as_ref().ok_or_else(|| {
            ControlPlaneStartupError::Smtp("SMTP transport was not composed".into())
        })?;
        let delivery_service = Arc::new(
            SmtpRecipientContactVerificationDeliveryService::from_transport(
                sender.clone(),
                Arc::clone(transport),
            ),
        );
        let dispatcher = Arc::new(
            RecipientContactVerificationDeliveryDispatcher::new(
                recipient_contact_verification_deliveries,
                Arc::clone(&recipient_contact_proof),
                delivery_service,
                chrono_duration(config.smtp.reservation_lease_ms)?,
            )
            .map_err(ControlPlaneStartupError::Smtp)?,
        );
        Some(
            A3sEventRecipientContactVerificationConsumer::new(
                event_publisher.bus(),
                event_publisher.subject(RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY),
                dispatcher,
            )
            .map_err(ControlPlaneStartupError::Smtp)?,
        )
    } else {
        None
    };
    let sources = adapters.sources.sources;
    let source_webhooks = adapters.sources.webhooks;
    let source_subscriptions = adapters.sources.subscriptions;
    let github_connections = adapters.sources.github_connections;
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
        managed_state_file: config.edge.managed_state_file.clone(),
    })
    .map_err(ControlPlaneStartupError::NodeControl)?;
    let mcp_projection_inputs = Arc::new(McpRouteProjectionInputReader::new(
        Arc::clone(&mcp_route_policy_repository),
        Arc::clone(&routes),
        Arc::clone(&mcp_profiles),
        Arc::clone(&workloads),
    ));
    let mcp_route_planner = McpRouteProjectionPlanner::new(
        Arc::clone(&route_targets),
        McpRouteTargetProjectionCompiler,
    );
    let mcp_projection_set_planner = Arc::new(McpGatewayProjectionSetPlanner::new(
        mcp_projection_inputs,
        McpGatewayProjectionPlanner::new(mcp_route_planner, mcp_credential_reader),
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
    let deployment_route_updates: Arc<dyn IDeploymentRouteUpdater> = Arc::new(
        EdgeDeploymentRouteUpdater::new_managed(
            Arc::clone(&routes),
            Arc::clone(&mcp_gateway_snapshots),
            Arc::clone(&node_control),
            Arc::clone(&route_commands),
            deployment_route_compiler.clone(),
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
    let durable_cell_artifacts = Arc::clone(&artifacts);
    let operation_interval = Duration::from_millis(config.operations.reconcile_interval_ms);
    let operation_lease = Duration::from_millis(config.operations.lease_ms);
    let flow = if run_operations {
        let source_checkout: Arc<dyn ISourceCheckout> = Arc::new(
            GitSourceCheckout::new(
                &config.sources.checkout_dir,
                Duration::from_millis(config.sources.checkout_timeout_ms),
                config.sources.checkout_max_files,
                config.sources.checkout_max_bytes,
            )
            .map_err(ControlPlaneStartupError::Build)?,
        );
        let source_build_inputs: Arc<dyn ISourceBuildInputQueryPort> =
            Arc::new(SourceBuildInputQueryService::new(Arc::clone(&sources)));
        let hosted_asset_build_inputs: Arc<dyn IHostedAssetBuildInputQueryPort> =
            Arc::new(HostedAssetBuildInputQueryService::new(
                Arc::clone(&assets),
                Arc::clone(&asset_git_repositories),
            ));
        let build_sources: Arc<dyn IBuildSourceResolver> = Arc::new(CloudBuildSourceResolver::new(
            source_build_inputs,
            hosted_asset_build_inputs,
        ));
        let external_source_archives = Arc::new(
            ExternalSourceBuildArchiveAdapter::new(
                source_checkout,
                Arc::clone(&github_connections),
                Arc::clone(&github_installation_tokens),
                &config.builds.input_staging_dir,
                config.builds.input_max_entries,
                config.builds.input_max_bytes,
            )
            .map_err(ControlPlaneStartupError::Build)?,
        );
        let build_inputs: Arc<dyn IBuildInputPreparer> = Arc::new(
            SourceBuildInputPreparer::new(external_source_archives, Arc::clone(&node_artifacts))
                .with_hosted_assets(Arc::clone(&assets), Arc::clone(&asset_git_repositories)),
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
        let build_evidence_signer =
            build_evidence_signer(&config, vault_credentials.as_ref()).await?;
        let build_evidence: Arc<dyn IBuildEvidenceGenerator> = Arc::new(
            BoxBuildEvidenceGenerator::new(oci_build_outputs, build_evidence_signer)
                .map_err(ControlPlaneStartupError::Build)?,
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
        let workload_prestart_gate: Arc<dyn IWorkloadPrestartGate> =
            Arc::new(DurableCellBundlePublicationGate::new(
                Arc::clone(&durable_cell_applications),
                Arc::clone(&durable_cell_deployments),
                Arc::clone(&builds),
                Arc::clone(&workloads),
                DurableCellPriorWriterSeal::new(
                    Arc::clone(&writer_fences),
                    Arc::clone(&operation_repository),
                ),
                Arc::clone(&environments),
                Arc::clone(&executions),
            ));
        let deployment_runtime = DeploymentFlowRuntime::new(
            DeploymentFlowDependencies::new(
                deployment_workloads,
                Arc::clone(&resource_claims),
                artifacts,
                scheduling_nodes,
                Arc::clone(&node_control),
                deployment_route_updates,
            )
            .with_prestart_gate(workload_prestart_gate),
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
        let object_namespace_recovery_runtime =
            ObjectNamespaceRecoveryFlowRuntime::new(ObjectNamespaceCredentialMaterializer::new(
                Arc::clone(&secrets),
                Arc::clone(&key_encryption),
            ))
            .map_err(ControlPlaneStartupError::ObjectNamespaceRecovery)?;
        let workflow_connector_responses: Arc<dyn IConnectorResponseObjectPort> =
            connector_execution.as_ref().cloned().ok_or_else(|| {
                ControlPlaneStartupError::Connector(
                    "Flow worker is missing Connector response-object access".into(),
                )
            })?;
        let flow_runtime = FlowRuntimeRouter::new(
            Arc::new(deployment_runtime),
            Arc::new(build_runtime),
            Arc::new(execution_runtime),
            Arc::new(agent_execution_runtime),
            Arc::new(WorkflowRunFlowRuntime::with_connector_responses(
                workflow_connector_responses,
            )),
            Arc::new(object_namespace_recovery_runtime),
        )?;
        Some(
            crate::infrastructure::connect_flow(
                &serving_postgres_url,
                Arc::new(flow_runtime),
                QueueOptions::new()
                    .with_poll_interval(operation_interval)
                    .with_lease_duration(operation_lease),
            )
            .await?,
        )
    } else {
        None
    };
    let management_flow_reader = if management_adapters.is_some() && flow.is_none() {
        Some(FlowReadInfrastructure::connect(&serving_postgres_url).await?)
    } else {
        None
    };
    let management_flow_engine = if management_adapters.is_some() {
        flow.as_ref()
            .map(crate::infrastructure::FlowInfrastructure::engine)
            .or_else(|| {
                management_flow_reader
                    .as_ref()
                    .map(FlowReadInfrastructure::engine)
            })
    } else {
        None
    };
    let workflow_run_history: Option<Arc<dyn IWorkflowRunHistoryReader>> = management_flow_engine
        .as_ref()
        .map(|engine| Arc::new(WorkflowRunHistoryReader::new(engine.clone())) as Arc<_>);
    let workflow_run_diagnostics: Option<Arc<dyn IWorkflowRunDiagnosticsReader>> =
        management_flow_engine
            .as_ref()
            .map(|engine| Arc::new(WorkflowRunDiagnosticsReader::new(engine.clone())) as Arc<_>);
    let workflow_run_variables: Option<Arc<dyn IWorkflowRunVariableReader>> =
        management_flow_engine
            .as_ref()
            .map(|engine| Arc::new(WorkflowRunVariableReader::new(engine.clone())) as Arc<_>);
    let worker_workflow = if let Some(flow) = flow.as_ref() {
        let workflow_execution_environments = Arc::clone(&environments);
        let workflow_execution_port: Arc<dyn IWorkflowExecutionPort> =
            Arc::new(WorkflowExecutionApplicationService::new(
                workflow_execution_environments,
                Arc::clone(&execution_templates),
                Arc::clone(&executions),
            ));
        let workflow_composite_port: Arc<dyn IWorkflowCompositeExecutionPort> =
            Arc::new(WorkflowCompositeExecutionApplicationService::new(
                Arc::clone(&workflow_definitions),
                Arc::clone(&ontologies),
                Arc::clone(&workflow_goals),
                Arc::clone(&workflow_runs),
            ));
        let workflow_connector_port: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort> =
            Arc::new(WorkflowConnectorApplicationService::new(Arc::clone(
                connector_execution.as_ref().ok_or_else(|| {
                    ControlPlaneStartupError::Connector(
                        "Flow worker is missing Connector execution".into(),
                    )
                })?,
            )));
        let workflow_application_effects: Arc<dyn IWorkflowApplicationEffectsPort> = Arc::new(
            WorkflowApplicationEffectsService::new(Arc::clone(&application_sessions)),
        );
        let workflow_run_coordinator: Arc<dyn IWorkflowRunCoordinator> = Arc::new(
            FlowWorkflowRunCoordinator::with_all_ports_and_application_effects(
                flow.engine(),
                workflow_execution_port,
                workflow_composite_port,
                workflow_connector_port,
                workflow_application_effects,
            ),
        );
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
        Some((
            workflow_run_reconciler,
            human_task_coordinator,
            human_task_resume_worker,
        ))
    } else {
        None
    };
    let management = if let Some(ManagementAdapterOverrides {
        source_resolver,
        oidc_provider,
    }) = management_adapters
    {
        let source_webhook_verifier: Arc<dyn ISourceWebhookVerifier> = Arc::new(
            GithubWebhookVerifier::new(
                config.sources.github_webhook_secret_env.clone(),
                config.sources.github_webhook_max_body_bytes,
            )
            .map_err(ControlPlaneStartupError::Sources)?,
        );
        let certificate_authority =
            certificate_authority_provider(&config, vault_credentials.as_ref())?;
        let bootstrap_credential = BootstrapCredential::new(&config.bootstrap_token()?)
            .map_err(ControlPlaneStartupError::Auth)?;
        let plugin_trust_roots: Arc<dyn IPluginTrustRootStore> = Arc::new(
            PluginTrustRootObjectStore::from_client(
                object_storage
                    .subnamespace("plugin-trust-roots")
                    .map_err(|error| ControlPlaneStartupError::Plugins(error.to_string()))?,
                MAX_BOOTSTRAP_ROOT_BYTES,
            )
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
        let mcp_service_profiles = Arc::new(McpServiceProfileApplicationService::new(
            Arc::clone(&mcp_profiles),
            Arc::clone(&assets),
        ));
        let mcp_route_policies = Arc::new(McpRoutePolicyApplicationService::new(
            mcp_route_policy_repository,
            Arc::clone(&mcp_profiles),
        ));
        let asset_git = Arc::new(
            AssetGitApplicationService::new(
                Arc::clone(&assets),
                Arc::clone(&asset_git_repositories),
                asset_controls,
                AssetGitApplicationServiceOptions {
                    write_lease: Duration::from_millis(config.assets.write_lease_ms),
                    default_repository_quota_bytes: config.assets.repository_quota_bytes,
                    maximum_rpc_body_bytes: u64::try_from(config.assets.max_rpc_body_bytes)
                        .map_err(|_| {
                            ControlPlaneStartupError::Assets(
                                "Asset Git RPC body bound exceeds u64".into(),
                            )
                        })?,
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
        Some(ManagementSurfaceDependencies {
            oidc_provider,
            plugin_trust_roots,
            plugin_catalog,
            asset_catalog,
            mcp_service_profiles,
            mcp_route_policies,
            asset_git,
            github_authorization,
            source_resolver,
            source_webhook_verifier,
            domain_verifier,
            gateway_projector,
            certificate_authority,
            bootstrap_credential,
        })
    } else {
        None
    };
    let node_control_server = if let Some(management) = management.as_ref() {
        let api = NodeControlApi::new(
            Arc::clone(&nodes),
            Arc::clone(&node_control),
            Arc::clone(&agents),
            Arc::clone(&node_artifacts),
            Arc::clone(&management.gateway_projector),
            Arc::clone(&routes),
            Arc::clone(&gateway_certificate_authority),
            Arc::clone(&log_chunks),
            Arc::clone(&management.certificate_authority),
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
    let outbox_relay = if run_relay {
        let developer_workflows = developer_workflow_projection.ok_or_else(|| {
            ControlPlaneStartupError::Framework(BootError::Internal(
                "relay process is missing its Developer Workflows projection adapters".into(),
            ))
        })?;
        Some(build_outbox_relay(
            &config,
            OutboxRelayDependencies {
                outbox: postgres_adapters.outbox(),
                events: event_publisher.clone().ok_or_else(|| {
                    ControlPlaneStartupError::Framework(BootError::Internal(
                        "relay process is missing its event publisher".into(),
                    ))
                })?,
                projectors: build_outbox_projectors(
                    Arc::clone(&notifications),
                    Arc::clone(&assets),
                    Arc::clone(&memberships),
                    Arc::clone(&alert_policies),
                    Arc::clone(&resource_grants),
                    Arc::clone(&build_candidates),
                    DeveloperWorkflowProjectionDependencies {
                        policies: developer_workflows.preview_policies,
                        previews: developer_workflows.preview_projections,
                        environments: Arc::clone(&environments),
                    },
                ),
            },
        )?)
    } else {
        None
    };
    let worker_gateway = if run_operations {
        let gateway_observations: Arc<dyn IGatewayObservationQueue> =
            Arc::new(FleetGatewayObservationQueue::new(Arc::clone(&node_control)));
        Some(WorkerGatewayDependencies {
            gateway_certificate_reconciler: GatewayCertificateReconciler::new_managed(
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
            .map_err(ControlPlaneStartupError::Edge)?,
            mcp_gateway_desired_state_reconciler: McpGatewayDesiredStateReconciler::new(
                Arc::clone(&mcp_gateway_snapshots),
                Arc::clone(&mcp_node_projection_planner),
                deployment_route_compiler.clone(),
                Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
                chrono_duration(config.edge.command_ttl_ms)?,
                chrono::Duration::hours(24),
                chrono_duration(config.edge.certificate_renewal_window_ms)?,
                chrono_duration(config.edge.command_ttl_ms)?,
                100,
            )
            .map_err(ControlPlaneStartupError::Edge)?,
            mcp_gateway_snapshot_reconciler: McpGatewaySnapshotReconciler::new(
                Arc::clone(&mcp_gateway_snapshots),
                Arc::clone(&route_commands),
                Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
                100,
            )
            .map_err(ControlPlaneStartupError::Edge)?,
            mcp_credential_delivery_receipt_sweeper: McpCredentialDeliveryReceiptSweeper::new(
                Arc::clone(&mcp_credentials),
                Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
                100,
            )
            .map_err(ControlPlaneStartupError::Edge)?,
            gateway_rollout_reconciler: GatewayRolloutReconciler::new(
                Arc::clone(&routes),
                Arc::clone(&route_commands),
                Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
                100,
            )
            .map_err(ControlPlaneStartupError::Edge)?,
            gateway_replica_recovery_reconciler: GatewayReplicaRecoveryReconciler::new(
                Arc::clone(&routes),
                gateway_observations,
                Duration::from_millis(config.edge.certificate_reconciliation_interval_ms),
                chrono_duration(config.edge.command_ttl_ms)?,
                100,
            )
            .map_err(ControlPlaneStartupError::Edge)?,
            gateway_rollout_rollback_reconciler: GatewayRolloutRollbackReconciler::new_managed(
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
            .map_err(ControlPlaneStartupError::Edge)?,
        })
    } else {
        None
    };
    let worker_processes = if let Some(flow) = flow.as_ref() {
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
            100,
        );
        let operation_coordinator = crate::infrastructure::FlowOperationCoordinator::new(
            operation_reconciler,
            flow,
            operation_interval,
            operation_lease,
        )
        .map_err(|error| {
            ControlPlaneStartupError::Framework(BootError::Internal(error.to_string()))
        })?;
        let log_retention_worker = LogRetentionWorker::new(
            Arc::clone(&log_retention_repository),
            Arc::clone(&log_chunks),
            Duration::from_millis(config.logs.retention_ms),
            Duration::from_millis(config.logs.retention_poll_ms),
            config.logs.retention_batch_size,
        )
        .map_err(ControlPlaneStartupError::LogMaintenance)?;
        let audit_retention_worker = AuditRetentionWorker::new(
            audit_retention_repository,
            Duration::from_millis(config.audit.retention_ms),
            Duration::from_millis(config.audit.retention_poll_ms),
            config.audit.retention_organization_batch_size,
            config.audit.retention_record_batch_size,
        )
        .map_err(ControlPlaneStartupError::AuditMaintenance)?;
        let log_compaction_worker = LogCompactionWorker::new(
            log_retention_repository,
            Duration::from_millis(config.logs.tombstone_retention_ms),
            Duration::from_millis(config.logs.tombstone_compaction_poll_ms),
            config.logs.tombstone_compaction_batch_size,
        )
        .map_err(ControlPlaneStartupError::LogMaintenance)?;
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
        let node_availability_reconciler = NodeAvailabilityReconciler::new(
            node_availability,
            Duration::from_millis(config.fleet.heartbeat_interval_ms),
            chrono_duration(config.fleet.heartbeat_timeout_ms)?,
            100,
        )
        .map_err(ControlPlaneStartupError::NodeControl)?;
        let durable_cell_writer_fences = Arc::new(DurableCellWriterFenceAdapter::new(
            Arc::clone(&durable_cell_applications),
            Arc::clone(&durable_cell_deployments),
            Arc::clone(&workloads),
            writer_fences,
            Arc::clone(&operation_repository),
        ));
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
        .map_err(ControlPlaneStartupError::NodeControl)?
        .with_writer_fence_adapter(durable_cell_writer_fences);
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
        let (workflow_run_reconciler, human_task_coordinator, human_task_resume_worker) =
            worker_workflow.ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "worker process is missing its workflow capability bundle".into(),
                ))
            })?;
        let WorkerGatewayDependencies {
            gateway_certificate_reconciler,
            mcp_gateway_desired_state_reconciler,
            mcp_gateway_snapshot_reconciler,
            mcp_credential_delivery_receipt_sweeper,
            gateway_rollout_reconciler,
            gateway_replica_recovery_reconciler,
            gateway_rollout_rollback_reconciler,
        } = worker_gateway.ok_or_else(|| {
            ControlPlaneStartupError::Framework(BootError::Internal(
                "worker process is missing its Gateway capability bundle".into(),
            ))
        })?;
        Some(ControlPlaneWorkers::worker(
            build_run_reconciler,
            execution_reconciler,
            agent_execution_reconciler,
            workflow_run_reconciler,
            human_task_coordinator,
            human_task_resume_worker,
            github_authority_reconciler,
            operation_coordinator,
            gateway_certificate_reconciler,
            mcp_gateway_desired_state_reconciler,
            mcp_gateway_snapshot_reconciler,
            mcp_credential_delivery_receipt_sweeper,
            gateway_rollout_reconciler,
            gateway_replica_recovery_reconciler,
            gateway_rollout_rollback_reconciler,
            secret_rotation_restart_reconciler,
            node_availability_reconciler,
            node_drain_evacuation_reconciler,
            replica_deployment_materializer,
            replica_retirement_reconciler,
            workload_reconciler,
            audit_retention_worker,
            log_retention_worker,
            log_compaction_worker,
            outbound_notification_consumer,
            recipient_contact_verification_consumer,
        ))
    } else {
        None
    };
    let readiness = match management.as_ref() {
        Some(management) if run_operations => infrastructure_readiness(
            executor,
            flow.clone().ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "worker process is missing Flow execution infrastructure".into(),
                ))
            })?,
            event_publisher.clone().ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "worker process is missing its event publisher".into(),
                ))
            })?,
            Arc::clone(&management.certificate_authority),
            Arc::clone(&gateway_certificate_authority),
            Arc::clone(&key_encryption),
            object_storage.clone(),
        ),
        Some(management) => api_readiness(
            executor,
            management_flow_reader.clone().ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "API process is missing Flow read infrastructure".into(),
                ))
            })?,
            Arc::clone(&management.certificate_authority),
            Arc::clone(&gateway_certificate_authority),
            Arc::clone(&key_encryption),
            object_storage.clone(),
        ),
        None => worker_readiness(
            executor,
            flow.clone().ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "worker process is missing Flow execution infrastructure".into(),
                ))
            })?,
            event_publisher.ok_or_else(|| {
                ControlPlaneStartupError::Framework(BootError::Internal(
                    "worker process is missing its event publisher".into(),
                ))
            })?,
            Arc::clone(&gateway_certificate_authority),
            Arc::clone(&key_encryption),
            object_storage,
        ),
    };
    let application = if let Some(management) = management {
        build_management_application_with_health(
            config.clone(),
            ManagementApplicationDependencies {
                management,
                organizations,
                api_tokens,
                memberships,
                membership_invitations,
                resource_grants,
                oidc_identity,
                recipient_contacts,
                recipient_contact_proof,
                resource_authorization_decisions,
                projects: projects.clone(),
                environments,
                ontologies,
                workflow_definitions,
                workflow_goals,
                workflow_runs,
                human_tasks,
                workflow_run_diagnostics: workflow_run_diagnostics.ok_or_else(|| {
                    ControlPlaneStartupError::Framework(BootError::Internal(
                        "management process is missing its Flow diagnostics reader".into(),
                    ))
                })?,
                workflow_run_history: workflow_run_history.ok_or_else(|| {
                    ControlPlaneStartupError::Framework(BootError::Internal(
                        "management process is missing its Flow history reader".into(),
                    ))
                })?,
                workflow_run_variables: workflow_run_variables.ok_or_else(|| {
                    ControlPlaneStartupError::Framework(BootError::Internal(
                        "management process is missing its Flow variable reader".into(),
                    ))
                })?,
                forms,
                form_semantic_core,
                search,
                audit_records,
                audit_export_signer: audit_export_signer.ok_or_else(|| {
                    ControlPlaneStartupError::Framework(BootError::Internal(
                        "management process is missing its audit export signer".into(),
                    ))
                })?,
                security_investigations,
                notifications,
                alert_policies,
                outbound_notifications,
                connector_profiles,
                connector_attempts,
                connector_attempt_resolutions,
                connector_revocations,
                applications,
                application_sessions,
                durable_cell_applications,
                durable_cell_deployments,
                oci_artifacts: durable_cell_artifacts,
                plugin_registries,
                plugin_enrollment_authorizer,
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
                github_installation_tokens,
                secret_encryption: Arc::clone(&key_encryption),
                route_targets,
                route_commands,
                mcp_gateway_snapshots: Some(mcp_gateway_snapshots),
                gateway_node_desired_state_planner: Some(gateway_node_desired_state_planner),
                operations: operation_repository,
                nodes,
                node_pools,
                node_control,
                log_chunks,
                readiness,
            },
        )?
    } else {
        build_process_status_application(&config, readiness)?
    };
    Ok(ControlPlane::new(application, {
        let mut workers = worker_processes.unwrap_or_default();
        if let Some(outbox_relay) = outbox_relay {
            workers = workers.with_relay(outbox_relay);
        }
        if let Some(node_control_server) = node_control_server {
            workers = workers.with_node_control(node_control_server);
        }
        workers
    }))
}

async fn build_relay_application(
    config: CloudConfig,
) -> std::result::Result<ControlPlane, ControlPlaneStartupError> {
    let serving_postgres_url = config.serving_postgres_url()?;
    let executor = connect_postgres(&serving_postgres_url, config.postgres.max_connections).await?;
    let event_publisher = event_publisher(&config).await?;
    let RelayPostgresAdapters {
        memberships,
        resource_grants,
        notifications,
        assets,
        build_candidates,
        environments,
        preview_policies,
        preview_projections,
        alert_policies,
        outbox,
    } = PostgresAdapterFactory::new(executor.clone()).relay();
    let outbox_relay = build_outbox_relay(
        &config,
        OutboxRelayDependencies {
            outbox,
            events: event_publisher.clone(),
            projectors: build_outbox_projectors(
                notifications,
                assets,
                memberships,
                alert_policies,
                resource_grants,
                build_candidates,
                DeveloperWorkflowProjectionDependencies {
                    policies: preview_policies,
                    previews: preview_projections,
                    environments,
                },
            ),
        },
    )?;
    let readiness = relay_readiness(executor, event_publisher);
    let application = build_process_status_application(&config, readiness)?;
    Ok(ControlPlane::new(
        application,
        ControlPlaneWorkers::relay(outbox_relay),
    ))
}

struct OutboxRelayDependencies {
    outbox: Arc<dyn IOutboxRepository>,
    events: Arc<dyn IEventPublisher>,
    projectors: Vec<Arc<dyn IIntegrationEventProjector>>,
}

fn build_outbox_relay(
    config: &CloudConfig,
    dependencies: OutboxRelayDependencies,
) -> std::result::Result<OutboxRelay, ControlPlaneStartupError> {
    let relay = OutboxRelay::new(
        dependencies.outbox,
        dependencies.events,
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
    Ok(dependencies
        .projectors
        .into_iter()
        .fold(relay, OutboxRelay::with_projector))
}

struct DeveloperWorkflowProjectionDependencies {
    policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
    previews: Arc<dyn IPullRequestPreviewProjectionRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
}

fn build_outbox_projectors(
    notifications: Arc<dyn INotificationRepository>,
    assets: Arc<dyn IAssetRepository>,
    memberships: Arc<dyn IMembershipRepository>,
    alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
    build_candidates: Arc<dyn IBuildCandidateProjectionPort>,
    developer_workflows: DeveloperWorkflowProjectionDependencies,
) -> Vec<Arc<dyn IIntegrationEventProjector>> {
    let preview_service: Arc<dyn IPullRequestPreviewProjectionPort> =
        Arc::new(PullRequestPreviewProjectionService::new(
            developer_workflows.policies,
            developer_workflows.previews,
        ));
    let preview_environments: Arc<dyn IPreviewEnvironmentPort> = Arc::new(
        ProjectsPreviewEnvironmentAdapter::new(developer_workflows.environments),
    );
    vec![
        Arc::new(
            OutboxNotificationProjector::new(notifications, memberships)
                .with_alert_policies(alert_policies, resource_grants),
        ),
        Arc::new(HostedBuildOutcomeProjector::new(assets)),
        Arc::new(BuildCandidateProjector::new(build_candidates)),
        Arc::new(PullRequestPreviewProjector::new(
            preview_service,
            preview_environments,
        )),
    ]
}

struct ManagementSurfaceDependencies {
    oidc_provider: Arc<dyn IOidcProviderService>,
    plugin_trust_roots: Arc<dyn IPluginTrustRootStore>,
    plugin_catalog: Arc<dyn IPluginRegistryCatalog>,
    asset_catalog: Arc<AssetCatalogApplicationService>,
    mcp_service_profiles: Arc<McpServiceProfileApplicationService>,
    mcp_route_policies: Arc<McpRoutePolicyApplicationService>,
    asset_git: Arc<AssetGitApplicationService>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    source_resolver: Arc<dyn ISourceResolver>,
    source_webhook_verifier: Arc<dyn ISourceWebhookVerifier>,
    domain_verifier: Arc<dyn IDomainOwnershipVerifier>,
    gateway_projector: Arc<dyn IGatewayAcknowledgementProjector>,
    certificate_authority: Arc<dyn ICertificateAuthority>,
    bootstrap_credential: BootstrapCredential,
}

struct WorkerGatewayDependencies {
    gateway_certificate_reconciler: GatewayCertificateReconciler,
    mcp_gateway_desired_state_reconciler: McpGatewayDesiredStateReconciler,
    mcp_gateway_snapshot_reconciler: McpGatewaySnapshotReconciler,
    mcp_credential_delivery_receipt_sweeper: McpCredentialDeliveryReceiptSweeper,
    gateway_rollout_reconciler: GatewayRolloutReconciler,
    gateway_replica_recovery_reconciler: GatewayReplicaRecoveryReconciler,
    gateway_rollout_rollback_reconciler: GatewayRolloutRollbackReconciler,
}

struct ManagementApplicationDependencies {
    management: ManagementSurfaceDependencies,
    organizations: Arc<dyn IOrganizationRepository>,
    api_tokens: Arc<dyn IApiTokenRepository>,
    memberships: Arc<dyn IMembershipRepository>,
    membership_invitations: Arc<dyn IMembershipInvitationRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
    oidc_identity: Arc<dyn IOidcIdentityRepository>,
    recipient_contacts: Arc<dyn IRecipientContactRepository>,
    recipient_contact_proof: Arc<dyn IRecipientContactProofService>,
    resource_authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
    workflow_definitions: Arc<dyn IWorkflowDefinitionRepository>,
    workflow_goals: Arc<dyn IWorkflowGoalRepository>,
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
    human_tasks: Arc<dyn IHumanTaskRepository>,
    workflow_run_diagnostics: Arc<dyn IWorkflowRunDiagnosticsReader>,
    workflow_run_history: Arc<dyn IWorkflowRunHistoryReader>,
    workflow_run_variables: Arc<dyn IWorkflowRunVariableReader>,
    forms: Arc<dyn IFormRepository>,
    form_semantic_core: Arc<dyn IFormSemanticCore>,
    search: Arc<dyn ISearchRepository>,
    audit_records: Arc<dyn IAuditRecordRepository>,
    audit_export_signer: Arc<dyn IAuditExportSigner>,
    security_investigations: Arc<dyn IGatewayRoutePolicyTimelineRepository>,
    notifications: Arc<dyn INotificationRepository>,
    alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
    outbound_notifications: Arc<dyn IOutboundNotificationRepository>,
    connector_profiles: Arc<dyn IConnectorProfileRepository>,
    connector_attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    connector_attempt_resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>,
    connector_revocations: Arc<dyn IConnectorRevisionRevocationRepository>,
    applications: Arc<dyn IApplicationRepository>,
    application_sessions: Arc<dyn IApplicationSessionRepository>,
    durable_cell_applications: Arc<dyn IDurableCellApplicationRepository>,
    durable_cell_deployments: Arc<dyn IDurableCellDeploymentRepository>,
    oci_artifacts: Arc<dyn IOciArtifactResolver>,
    plugin_registries: Arc<dyn IPluginRegistryRepository>,
    plugin_enrollment_authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer>,
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
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    secret_encryption: Arc<dyn ISecretEncryptionService>,
    route_targets: Arc<dyn IRouteTargetReader>,
    route_commands: Arc<dyn IGatewayCommandQueue>,
    mcp_gateway_snapshots: Option<Arc<dyn crate::modules::edge::IMcpGatewaySnapshotRepository>>,
    gateway_node_desired_state_planner: Option<GatewayNodeDesiredStatePlanner>,
    operations: Arc<dyn IOperationRepository>,
    nodes: Arc<dyn INodeRepository>,
    node_pools: Arc<dyn INodePoolRepository>,
    node_control: Arc<dyn INodeControlRepository>,
    log_chunks: Arc<dyn ILogChunkStore>,
    readiness: HealthModule,
}

fn build_management_application_with_health(
    config: CloudConfig,
    dependencies: ManagementApplicationDependencies,
) -> Result<BootApplication> {
    if !config.server.role.serves_management_api() {
        return Err(BootError::Internal(
            "a non-management process cannot acquire management dependencies".into(),
        ));
    }
    let audit_retention_policy =
        AuditRetentionPolicy::new(Duration::from_millis(config.audit.retention_ms))
            .map_err(BootError::Internal)?;

    let ManagementApplicationDependencies {
        management,
        organizations,
        api_tokens,
        memberships,
        membership_invitations,
        resource_grants,
        oidc_identity,
        recipient_contacts,
        recipient_contact_proof,
        resource_authorization_decisions,
        projects,
        environments,
        ontologies,
        workflow_definitions,
        workflow_goals,
        workflow_runs,
        human_tasks,
        workflow_run_diagnostics,
        workflow_run_history,
        workflow_run_variables,
        forms,
        form_semantic_core,
        search,
        audit_records,
        audit_export_signer,
        security_investigations,
        notifications,
        alert_policies,
        outbound_notifications,
        connector_profiles,
        connector_attempts,
        connector_attempt_resolutions,
        connector_revocations,
        applications,
        application_sessions,
        durable_cell_applications,
        durable_cell_deployments,
        oci_artifacts,
        plugin_registries,
        plugin_enrollment_authorizer,
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
        github_installation_tokens,
        secret_encryption,
        route_targets,
        route_commands,
        mcp_gateway_snapshots,
        gateway_node_desired_state_planner,
        operations,
        nodes,
        node_pools,
        node_control,
        log_chunks,
        readiness,
    } = dependencies;
    let ManagementSurfaceDependencies {
        oidc_provider,
        plugin_trust_roots,
        plugin_catalog,
        asset_catalog,
        mcp_service_profiles,
        mcp_route_policies,
        asset_git,
        github_authorization,
        source_resolver,
        source_webhook_verifier,
        domain_verifier,
        gateway_projector,
        certificate_authority,
        bootstrap_credential,
    } = management;
    let operation_resource_access = Arc::new(OperationResourceAccessResolver::new(
        Arc::clone(&workloads),
        Arc::clone(&builds),
        Arc::clone(&executions),
        Arc::clone(&agents),
        Arc::clone(&workflow_runs),
    ));
    let list_notifications = Arc::clone(&notifications);
    let get_notifications = Arc::clone(&notifications);
    let mark_notifications_read = notifications;
    let create_notification_alert_policies = Arc::clone(&alert_policies);
    let revoke_notification_alert_policies = Arc::clone(&alert_policies);
    let list_notification_alert_policies = Arc::clone(&alert_policies);
    let get_notification_alert_policies = alert_policies;
    let notification_alert_policy_environments = Arc::clone(&environments);
    let notification_alert_policy_nodes = Arc::clone(&nodes);
    let create_outbound_notification_subscriptions = Arc::clone(&outbound_notifications);
    let revoke_outbound_notification_subscriptions = Arc::clone(&outbound_notifications);
    let list_outbound_notification_subscriptions = Arc::clone(&outbound_notifications);
    let get_outbound_notification_subscriptions = outbound_notifications;
    let create_connector_environments = Arc::clone(&environments);
    let outbound_notification_connector_profiles = Arc::clone(&connector_profiles);
    let outbound_notification_recipient_contacts = Arc::clone(&recipient_contacts);
    let create_connector_profiles = Arc::clone(&connector_profiles);
    let revise_connector_profiles = Arc::clone(&connector_profiles);
    let list_connector_profiles = Arc::clone(&connector_profiles);
    let get_connector_profiles = Arc::clone(&connector_profiles);
    let list_connector_revisions = Arc::clone(&connector_profiles);
    let get_connector_revisions = Arc::clone(&connector_profiles);
    let list_connector_execution_attempt_profiles = Arc::clone(&connector_profiles);
    let revoke_connector_revision_profiles = connector_profiles;
    let list_connector_execution_attempts = Arc::clone(&connector_attempts);
    let get_connector_execution_attempts = Arc::clone(&connector_attempts);
    let resolve_connector_execution_attempts = connector_attempts;
    let resolve_connector_execution_attempt_resolutions =
        Arc::clone(&connector_attempt_resolutions);
    let get_connector_execution_attempt_resolutions = connector_attempt_resolutions;
    let revoke_connector_revisions = Arc::clone(&connector_revocations);
    let get_connector_revision_revocations = connector_revocations;
    let workflow_definition_publications: Arc<dyn IWorkflowDefinitionPublicationPort> =
        Arc::new(WorkflowDefinitionPublicationService::new(
            Arc::clone(&projects),
            Arc::clone(&workflow_definitions),
        ));
    let application_workflow_evidence: Arc<dyn IApplicationWorkflowRevisionPort> = Arc::new(
        WorkflowApplicationReleaseEvidenceReader::new(Arc::clone(&workflow_definitions)),
    );
    let application_preset_workflows: Arc<dyn IApplicationPresetWorkflowPort> = Arc::new(
        WorkflowApplicationPresetCompiler::new(Arc::clone(&workflow_definition_publications)),
    );
    let application_ontology_evidence: Arc<dyn IApplicationOntologyRevisionPort> = Arc::new(
        WorkflowApplicationOntologyRevisionReader::new(Arc::clone(&ontologies)),
    );
    let application_workflow_runs: Arc<dyn IApplicationWorkflowRunPort> =
        Arc::new(WorkflowApplicationRunService::new(
            Arc::clone(&workflow_definitions),
            Arc::clone(&ontologies),
            Arc::clone(&workflow_goals),
            Arc::clone(&workflow_runs),
        ));
    let create_applications = Arc::clone(&applications);
    let publish_applications = Arc::clone(&applications);
    let compile_application_presets = application_preset_workflows;
    let compose_application_invocations = Arc::clone(&applications);
    let compose_application_sessions = Arc::clone(&application_sessions);
    let compose_application_workflow_runs = Arc::clone(&application_workflow_runs);
    let open_application_releases = Arc::clone(&applications);
    let open_application_session_records = Arc::clone(&application_sessions);
    let close_application_releases = Arc::clone(&applications);
    let close_application_session_records = Arc::clone(&application_sessions);
    let request_application_releases = Arc::clone(&applications);
    let request_application_invocation_records = Arc::clone(&application_sessions);
    let request_application_workflow_runs = Arc::clone(&application_workflow_runs);
    let cancel_application_releases = Arc::clone(&applications);
    let cancel_application_invocation_records = Arc::clone(&application_sessions);
    let cancel_application_workflow_runs = Arc::clone(&application_workflow_runs);
    let admit_application_releases = Arc::clone(&applications);
    let admit_application_sessions = Arc::clone(&application_sessions);
    let admit_application_invocation_releases = Arc::clone(&applications);
    let admit_application_invocation_sessions = Arc::clone(&application_sessions);
    let admit_application_environments = Arc::clone(&environments);
    let admit_application_workflow_runs = application_workflow_runs;
    let get_application_sessions = Arc::clone(&application_sessions);
    let get_application_invocations = Arc::clone(&application_sessions);
    let replay_application_sessions = application_sessions;
    let list_applications = Arc::clone(&applications);
    let get_applications = Arc::clone(&applications);
    let list_application_releases = Arc::clone(&applications);
    let get_application_releases = applications;
    let create_connector_secrets = Arc::clone(&secrets);
    let revise_connector_secrets = Arc::clone(&secrets);
    let create_durable_cell_environments = Arc::clone(&environments);
    let create_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let revise_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let start_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let stop_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let start_durable_cell_workloads = Arc::clone(&workloads);
    let stop_durable_cell_workloads = Arc::clone(&workloads);
    let list_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let get_durable_cell_applications = Arc::clone(&durable_cell_applications);
    let list_durable_cell_revisions = Arc::clone(&durable_cell_applications);
    let get_durable_cell_revisions = durable_cell_applications;
    let publish_durable_cell_deployments = Arc::clone(&durable_cell_deployments);
    let deploy_durable_cell_deployments = durable_cell_deployments;
    let deploy_durable_cell_workloads = Arc::clone(&workloads);
    let deploy_durable_cell_secrets = Arc::clone(&secrets);
    let deploy_durable_cell_node_pools = Arc::clone(&node_pools);
    let deploy_durable_cell_applications = Arc::clone(&get_durable_cell_applications);
    let create_durable_cell_builds = Arc::clone(&builds);
    let revise_durable_cell_builds = Arc::clone(&builds);
    let deploy_durable_cell_handler = DeployDurableCellApplicationHandler::new(
        deploy_durable_cell_applications,
        deploy_durable_cell_deployments,
        deploy_durable_cell_workloads,
        deploy_durable_cell_secrets,
        deploy_durable_cell_node_pools,
    );
    let deploy_durable_cell_from_acl_handler = DeployDurableCellApplicationFromAclHandler::new(
        oci_artifacts,
        deploy_durable_cell_handler.clone(),
    );
    let project_organizations = Arc::clone(&organizations);
    let create_projects = Arc::clone(&projects);
    let update_project_attributions = Arc::clone(&projects);
    let environment_projects = Arc::clone(&projects);
    let create_ontology_projects = Arc::clone(&projects);
    let create_ontologies = Arc::clone(&ontologies);
    let revise_ontologies = Arc::clone(&ontologies);
    let get_ontologies = Arc::clone(&ontologies);
    let list_ontologies = Arc::clone(&ontologies);
    let get_ontology_revisions = Arc::clone(&ontologies);
    let list_ontology_revisions = Arc::clone(&ontologies);
    let diff_ontology_revisions = Arc::clone(&ontologies);
    let get_workflow_node_catalog_projects = Arc::clone(&projects);
    let create_workflow_definition_publications = workflow_definition_publications;
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
    let get_workflow_run_variable_runs = Arc::clone(&workflow_runs);
    let get_workflow_run_diagnostics_runs = Arc::clone(&workflow_runs);
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
    let begin_oidc_organizations = Arc::clone(&organizations);
    let begin_oidc_memberships = Arc::clone(&memberships);
    let begin_oidc_identity = Arc::clone(&oidc_identity);
    let begin_oidc_provider = Arc::clone(&oidc_provider);
    let begin_recipient_contacts = Arc::clone(&recipient_contacts);
    let begin_recipient_contact_proof = Arc::clone(&recipient_contact_proof);
    let complete_recipient_contacts = Arc::clone(&recipient_contacts);
    let complete_recipient_contact_proof = Arc::clone(&recipient_contact_proof);
    let revoke_recipient_contacts = Arc::clone(&recipient_contacts);
    let list_recipient_contacts = Arc::clone(&recipient_contacts);
    let get_recipient_contacts = Arc::clone(&recipient_contacts);
    let create_memberships = Arc::clone(&memberships);
    let change_memberships = Arc::clone(&memberships);
    let revoke_memberships = Arc::clone(&memberships);
    let list_memberships = Arc::clone(&memberships);
    let get_memberships = Arc::clone(&memberships);
    let create_membership_invitations = Arc::clone(&membership_invitations);
    let accept_membership_invitations = Arc::clone(&membership_invitations);
    let revoke_membership_invitations = Arc::clone(&membership_invitations);
    let list_membership_invitations = Arc::clone(&membership_invitations);
    let get_membership_invitations = Arc::clone(&membership_invitations);
    let list_my_membership_invitations = Arc::clone(&membership_invitations);
    let create_resource_grants = Arc::clone(&resource_grants);
    let resource_grant_projects = Arc::clone(&projects);
    let resource_grant_environments = Arc::clone(&environments);
    let resource_grant_nodes = Arc::clone(&nodes);
    let revoke_resource_grants = Arc::clone(&resource_grants);
    let list_resource_grants = Arc::clone(&resource_grants);
    let get_resource_grants = Arc::clone(&resource_grants);
    let query_organizations = Arc::clone(&organizations);
    let query_projects = Arc::clone(&projects);
    let get_project_attributions = Arc::clone(&projects);
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
    let hosted_artifacts: Arc<dyn IHostedArtifactQueryPort> =
        Arc::new(HostedArtifactQueryService::new(Arc::clone(&builds)));
    let agent_create_artifacts = Arc::clone(&hosted_artifacts);
    let agent_update_artifacts = Arc::clone(&hosted_artifacts);
    let agent_execution_artifacts = hosted_artifacts;
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
        .import(process_liveness_module())
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
                .command_handler::<crate::modules::identity::CreateMembership, _>(
                    CreateMembershipHandler::new(create_memberships),
                )
                .command_handler::<crate::modules::identity::ChangeMembershipRole, _>(
                    ChangeMembershipRoleHandler::new(change_memberships),
                )
                .command_handler::<crate::modules::identity::RevokeMembership, _>(
                    RevokeMembershipHandler::new(revoke_memberships),
                )
                .command_handler::<crate::modules::identity::CreateMembershipInvitation, _>(
                    CreateMembershipInvitationHandler::new(create_membership_invitations),
                )
                .command_handler::<crate::modules::identity::AcceptMembershipInvitation, _>(
                    AcceptMembershipInvitationHandler::new(accept_membership_invitations),
                )
                .command_handler::<crate::modules::identity::RevokeMembershipInvitation, _>(
                    RevokeMembershipInvitationHandler::new(revoke_membership_invitations),
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
                .command_handler::<crate::modules::identity::BeginOidcFlow, _>(
                    BeginOidcFlowHandler::new(
                        begin_oidc_organizations,
                        begin_oidc_memberships,
                        begin_oidc_identity,
                        begin_oidc_provider,
                    ),
                )
                .command_handler::<crate::modules::identity::CompleteOidcFlow, _>(
                    CompleteOidcFlowHandler::new(oidc_identity, oidc_provider),
                )
                .command_handler::<
                    crate::modules::identity::BeginRecipientContactVerification,
                    _,
                >(BeginRecipientContactVerificationHandler::new(
                    begin_recipient_contacts,
                    begin_recipient_contact_proof,
                ))
                .command_handler::<
                    crate::modules::identity::CompleteRecipientContactVerification,
                    _,
                >(CompleteRecipientContactVerificationHandler::new(
                    complete_recipient_contacts,
                    complete_recipient_contact_proof,
                ))
                .command_handler::<crate::modules::identity::RevokeRecipientContact, _>(
                    RevokeRecipientContactHandler::new(revoke_recipient_contacts),
                )
                .command_handler::<crate::modules::projects::CreateProject, _>(
                    CreateProjectHandler::new(project_organizations, create_projects),
                )
                .command_handler::<crate::modules::projects::UpdateProjectAttribution, _>(
                    UpdateProjectAttributionHandler::new(update_project_attributions),
                )
                .command_handler::<crate::modules::notifications::MarkNotificationRead, _>(
                    MarkNotificationReadHandler::new(mark_notifications_read),
                )
                .command_handler::<
                    crate::modules::notifications::CreateNotificationAlertPolicy,
                    _,
                >(CreateNotificationAlertPolicyHandler::new(
                    create_notification_alert_policies,
                    notification_alert_policy_environments,
                    notification_alert_policy_nodes,
                ))
                .command_handler::<
                    crate::modules::notifications::RevokeNotificationAlertPolicy,
                    _,
                >(RevokeNotificationAlertPolicyHandler::new(
                    revoke_notification_alert_policies,
                ))
                .command_handler::<
                    crate::modules::notifications::CreateOutboundNotificationSubscription,
                    _,
                >(
                    CreateOutboundNotificationSubscriptionHandler::new(
                        create_outbound_notification_subscriptions,
                        outbound_notification_connector_profiles,
                        outbound_notification_recipient_contacts,
                    ),
                )
                .command_handler::<
                    crate::modules::notifications::RevokeOutboundNotificationSubscription,
                    _,
                >(
                    RevokeOutboundNotificationSubscriptionHandler::new(
                        revoke_outbound_notification_subscriptions,
                    ),
                )
                .command_handler::<crate::modules::connectors::CreateConnectorProfile, _>(
                    CreateConnectorProfileHandler::new(
                        create_connector_environments,
                        create_connector_profiles,
                        create_connector_secrets,
                    ),
                )
                .command_handler::<crate::modules::connectors::ReviseConnectorProfile, _>(
                    ReviseConnectorProfileHandler::new(
                        revise_connector_profiles,
                        revise_connector_secrets,
                    ),
                )
                .command_handler::<crate::modules::connectors::RevokeConnectorRevision, _>(
                    RevokeConnectorRevisionHandler::new(
                        revoke_connector_revision_profiles,
                        revoke_connector_revisions,
                    ),
                )
                .command_handler::<
                    crate::modules::connectors::ResolveConnectorExecutionAttempt,
                    _,
                >(ResolveConnectorExecutionAttemptHandler::new(
                    resolve_connector_execution_attempts,
                    resolve_connector_execution_attempt_resolutions,
                ))
                .command_handler::<crate::modules::applications::CreateApplication, _>(
                    CreateApplicationHandler::new(
                        create_applications,
                        Arc::clone(&application_workflow_evidence),
                    ),
                )
                .command_handler::<crate::modules::applications::PublishApplicationRelease, _>(
                    PublishApplicationReleaseHandler::new(
                        publish_applications,
                        application_workflow_evidence,
                    ),
                )
                .command_handler::<
                    crate::modules::applications::CompileApplicationPresetWorkflow,
                    _,
                >(CompileApplicationPresetWorkflowHandler::new(
                    compile_application_presets,
                ))
                .command_handler::<crate::modules::applications::OpenApplicationSession, _>(
                    OpenApplicationSessionHandler::new(
                        open_application_releases,
                        open_application_session_records,
                    ),
                )
                .command_handler::<crate::modules::applications::CloseApplicationSession, _>(
                    CloseApplicationSessionHandler::new(
                        close_application_releases,
                        close_application_session_records,
                    ),
                )
                .command_handler::<crate::modules::applications::RequestApplicationInvocation, _>(
                    RequestApplicationInvocationHandler::new(
                        request_application_releases,
                        request_application_invocation_records,
                        request_application_workflow_runs,
                    ),
                )
                .command_handler::<crate::modules::applications::CancelApplicationInvocation, _>(
                    CancelApplicationInvocationHandler::new(
                        cancel_application_releases,
                        cancel_application_invocation_records,
                        cancel_application_workflow_runs,
                    ),
                )
                .command_handler::<
                    crate::modules::applications::ComposeApplicationInvocationWorkflowRun,
                    _,
                >(ComposeApplicationInvocationWorkflowRunHandler::new(
                    compose_application_invocations,
                    compose_application_sessions,
                    compose_application_workflow_runs,
                ))
                .command_handler::<crate::modules::applications::AdmitApplicationSession, _>(
                    AdmitApplicationSessionHandler::new(
                        admit_application_releases,
                        admit_application_sessions,
                    ),
                )
                .command_handler::<crate::modules::applications::AdmitApplicationInvocation, _>(
                    AdmitApplicationInvocationHandler::new(
                        admit_application_invocation_releases,
                        admit_application_invocation_sessions,
                        application_ontology_evidence,
                        admit_application_environments,
                        admit_application_workflow_runs,
                    ),
                )
                .command_handler::<crate::modules::durable_cells::CreateDurableCellApplication, _>(
                    CreateDurableCellApplicationHandler::new(
                        create_durable_cell_environments,
                        create_durable_cell_applications,
                        create_durable_cell_builds,
                    ),
                )
                .command_handler::<crate::modules::durable_cells::ReviseDurableCellApplication, _>(
                    ReviseDurableCellApplicationHandler::new(
                        revise_durable_cell_applications,
                        revise_durable_cell_builds,
                    ),
                )
                .command_handler::<crate::modules::durable_cells::StartDurableCellApplication, _>(
                    StartDurableCellApplicationHandler::new(
                        start_durable_cell_applications,
                        start_durable_cell_workloads,
                    ),
                )
                .command_handler::<crate::modules::durable_cells::StopDurableCellApplication, _>(
                    StopDurableCellApplicationHandler::new(
                        stop_durable_cell_applications,
                        stop_durable_cell_workloads,
                    ),
                )
                .command_handler::<crate::modules::durable_cells::DeployDurableCellApplication, _>(
                    deploy_durable_cell_handler,
                )
                .command_handler::<
                    crate::modules::durable_cells::DeployDurableCellApplicationFromAcl,
                    _,
                >(
                    deploy_durable_cell_from_acl_handler,
                )
                .command_handler::<
                    crate::modules::durable_cells::PublishDurableCellApplicationRoute,
                    _,
                >(PublishDurableCellApplicationRouteHandler::new(
                    publish_durable_cell_deployments,
                    publish_route_handler.clone(),
                ))
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
                        create_workflow_definition_publications,
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
                        agent_create_artifacts,
                        agent_create_workloads,
                        agent_create_workload_secrets,
                        agent_workload_node_pools,
                    ),
                )
                .command_handler::<crate::modules::workloads::UpdateAgentWorkloadDeployment, _>(
                    UpdateAgentWorkloadDeploymentHandler::new(
                        agent_update_assets,
                        agent_update_artifacts,
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
                        agent_execution_artifacts,
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
                .query_handler::<crate::modules::identity::ListMembershipInvitations, _>(
                    ListMembershipInvitationsHandler::new(list_membership_invitations),
                )
                .query_handler::<crate::modules::identity::GetMembershipInvitation, _>(
                    GetMembershipInvitationHandler::new(get_membership_invitations),
                )
                .query_handler::<crate::modules::identity::ListMyMembershipInvitations, _>(
                    ListMyMembershipInvitationsHandler::new(list_my_membership_invitations),
                )
                .query_handler::<crate::modules::identity::ListResourceGrants, _>(
                    ListResourceGrantsHandler::new(list_resource_grants),
                )
                .query_handler::<crate::modules::identity::GetResourceGrant, _>(
                    GetResourceGrantHandler::new(get_resource_grants),
                )
                .query_handler::<crate::modules::identity::ListRecipientContacts, _>(
                    ListRecipientContactsHandler::new(list_recipient_contacts),
                )
                .query_handler::<crate::modules::identity::GetRecipientContact, _>(
                    GetRecipientContactHandler::new(get_recipient_contacts),
                )
                .query_handler::<crate::modules::projects::ListProjects, _>(
                    ListProjectsHandler::new(query_projects),
                )
                .query_handler::<crate::modules::projects::GetProjectAttribution, _>(
                    GetProjectAttributionHandler::new(get_project_attributions),
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
                .query_handler::<crate::modules::workflow::GetWorkflowNodeCatalog, _>(
                    GetWorkflowNodeCatalogHandler::new(get_workflow_node_catalog_projects),
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
                .query_handler::<crate::modules::workflow::GetWorkflowRunVariables, _>(
                    GetWorkflowRunVariablesHandler::new(
                        get_workflow_run_variable_runs,
                        workflow_run_variables,
                    ),
                )
                .query_handler::<crate::modules::workflow::GetWorkflowRunDiagnostics, _>(
                    GetWorkflowRunDiagnosticsHandler::new(
                        get_workflow_run_diagnostics_runs,
                        workflow_run_diagnostics,
                    ),
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
                .query_handler::<crate::modules::audit::ListAuditRecords, _>(
                    ListAuditRecordsHandler::new(Arc::clone(&audit_records)),
                )
                .query_handler::<crate::modules::audit::GetAuditRetentionStatus, _>(
                    GetAuditRetentionStatusHandler::new(
                        Arc::clone(&audit_records),
                        audit_retention_policy.clone(),
                    ),
                )
                .query_handler::<crate::modules::audit::ExportAuditManifest, _>(
                    ExportAuditManifestHandler::new(
                        Arc::clone(&audit_records),
                        Arc::clone(&audit_export_signer),
                        audit_retention_policy,
                    ),
                )
                .query_handler::<crate::modules::audit::ExportAuditRecords, _>(
                    ExportAuditRecordsHandler::new(audit_records, audit_export_signer),
                )
                .query_handler::<crate::modules::security::ListGatewayRoutePolicyTimeline, _>(
                    ListGatewayRoutePolicyTimelineHandler::new(security_investigations),
                )
                .query_handler::<crate::modules::notifications::ListNotifications, _>(
                    ListNotificationsHandler::new(list_notifications),
                )
                .query_handler::<crate::modules::notifications::GetNotification, _>(
                    GetNotificationHandler::new(get_notifications),
                )
                .query_handler::<
                    crate::modules::notifications::ListNotificationAlertPolicies,
                    _,
                >(ListNotificationAlertPoliciesHandler::new(
                    list_notification_alert_policies,
                ))
                .query_handler::<crate::modules::notifications::GetNotificationAlertPolicy, _>(
                    GetNotificationAlertPolicyHandler::new(get_notification_alert_policies),
                )
                .query_handler::<
                    crate::modules::notifications::ListOutboundNotificationSubscriptions,
                    _,
                >(
                    ListOutboundNotificationSubscriptionsHandler::new(
                        list_outbound_notification_subscriptions,
                    ),
                )
                .query_handler::<
                    crate::modules::notifications::GetOutboundNotificationSubscription,
                    _,
                >(
                    GetOutboundNotificationSubscriptionHandler::new(
                        get_outbound_notification_subscriptions,
                    ),
                )
                .query_handler::<crate::modules::connectors::ListConnectorProfiles, _>(
                    ListConnectorProfilesHandler::new(list_connector_profiles),
                )
                .query_handler::<crate::modules::connectors::GetConnectorProfile, _>(
                    GetConnectorProfileHandler::new(get_connector_profiles),
                )
                .query_handler::<crate::modules::connectors::ListConnectorRevisions, _>(
                    ListConnectorRevisionsHandler::new(list_connector_revisions),
                )
                .query_handler::<crate::modules::connectors::GetConnectorRevision, _>(
                    GetConnectorRevisionHandler::new(get_connector_revisions),
                )
                .query_handler::<
                    crate::modules::connectors::GetConnectorRevisionRevocation,
                    _,
                >(GetConnectorRevisionRevocationHandler::new(
                    get_connector_revision_revocations,
                ))
                .query_handler::<
                    crate::modules::connectors::ListUnresolvedConnectorExecutionAttempts,
                    _,
                >(ListUnresolvedConnectorExecutionAttemptsHandler::new(
                    list_connector_execution_attempt_profiles,
                    list_connector_execution_attempts,
                ))
                .query_handler::<crate::modules::connectors::GetConnectorExecutionAttempt, _>(
                    GetConnectorExecutionAttemptHandler::new(get_connector_execution_attempts),
                )
                .query_handler::<
                    crate::modules::connectors::GetConnectorExecutionAttemptResolution,
                    _,
                >(GetConnectorExecutionAttemptResolutionHandler::new(
                    get_connector_execution_attempt_resolutions,
                ))
                .query_handler::<crate::modules::applications::ListApplications, _>(
                    ListApplicationsHandler::new(list_applications),
                )
                .query_handler::<crate::modules::applications::GetApplication, _>(
                    GetApplicationHandler::new(get_applications),
                )
                .query_handler::<crate::modules::applications::ListApplicationReleases, _>(
                    ListApplicationReleasesHandler::new(list_application_releases),
                )
                .query_handler::<crate::modules::applications::GetApplicationRelease, _>(
                    GetApplicationReleaseHandler::new(get_application_releases),
                )
                .query_handler::<crate::modules::applications::GetApplicationSession, _>(
                    GetApplicationSessionHandler::new(get_application_sessions),
                )
                .query_handler::<crate::modules::applications::GetApplicationInvocation, _>(
                    GetApplicationInvocationHandler::new(get_application_invocations),
                )
                .query_handler::<crate::modules::applications::ReplayApplicationSession, _>(
                    ReplayApplicationSessionHandler::new(replay_application_sessions),
                )
                .query_handler::<crate::modules::durable_cells::ListDurableCellApplications, _>(
                    ListDurableCellApplicationsHandler::new(list_durable_cell_applications),
                )
                .query_handler::<crate::modules::durable_cells::GetDurableCellApplication, _>(
                    GetDurableCellApplicationHandler::new(get_durable_cell_applications),
                )
                .query_handler::<
                    crate::modules::durable_cells::ListDurableCellApplicationRevisions,
                    _,
                >(
                    ListDurableCellApplicationRevisionsHandler::new(
                        list_durable_cell_revisions,
                    ),
                )
                .query_handler::<
                    crate::modules::durable_cells::GetDurableCellApplicationRevision,
                    _,
                >(
                    GetDurableCellApplicationRevisionHandler::new(get_durable_cell_revisions),
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
        .import(AuditModule)
        .import(SecurityModule)
        .import(NotificationsModule)
        .import(ConnectorsModule)
        .import(ApplicationsModule)
        .import(DurableCellsModule)
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

fn process_liveness_module() -> PublicHealthModule {
    PublicHealthModule::new(
        HealthModule::new("health")
            .with_route("/health/live")
            .indicator("process", || async { Ok(HealthIndicatorResult::up()) }),
    )
}

fn build_process_status_application(
    config: &CloudConfig,
    readiness: HealthModule,
) -> Result<BootApplication> {
    BootApplication::builder()
        .import(process_liveness_module())
        .import(PublicHealthModule::new(readiness))
        .import(PlatformModule::new(config))
        .use_global_middleware(RequestIdMiddleware)
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
    object_storage: ImmutableObjectClient,
) -> HealthModule {
    worker_readiness(
        executor,
        flow,
        events,
        gateway_certificate_authority,
        key_encryption,
        object_storage,
    )
    .indicator("certificate-authority", move || {
        let certificate_authority = certificate_authority.clone();
        async move {
            match certificate_authority.health().await {
                Ok(true) => Ok(HealthIndicatorResult::up()),
                Ok(false) => Ok(HealthIndicatorResult::down()),
                Err(error) => {
                    Ok(HealthIndicatorResult::down().with_detail_value("error", error.to_string()))
                }
            }
        }
    })
}

fn api_readiness(
    executor: PostgresExecutor,
    flow: FlowReadInfrastructure,
    certificate_authority: Arc<dyn ICertificateAuthority>,
    gateway_certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
    key_encryption: Arc<dyn ISecretEncryptionService>,
    object_storage: ImmutableObjectClient,
) -> HealthModule {
    postgres_readiness(executor)
        .indicator("flow", move || {
            let flow = flow.clone();
            async move { Ok(flow.health().await) }
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
        .indicator("object-storage", move || {
            let object_storage = object_storage.clone();
            async move {
                match object_storage.health().await {
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

fn worker_readiness(
    executor: PostgresExecutor,
    flow: crate::infrastructure::FlowInfrastructure,
    events: Arc<dyn IEventPublisher>,
    gateway_certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
    key_encryption: Arc<dyn ISecretEncryptionService>,
    object_storage: ImmutableObjectClient,
) -> HealthModule {
    relay_readiness(executor, events)
        .indicator("flow", move || {
            let flow = flow.clone();
            async move { Ok(flow.health().await) }
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
        .indicator("object-storage", move || {
            let object_storage = object_storage.clone();
            async move {
                match object_storage.health().await {
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

fn relay_readiness(executor: PostgresExecutor, events: Arc<dyn IEventPublisher>) -> HealthModule {
    postgres_readiness(executor).indicator("events", move || {
        let events = events.clone();
        async move {
            match events.health().await {
                Ok(true) => Ok(HealthIndicatorResult::up()),
                Ok(false) => Ok(HealthIndicatorResult::down()),
                Err(error) => {
                    Ok(HealthIndicatorResult::down().with_detail_value("error", error.to_string()))
                }
            }
        }
    })
}

fn postgres_readiness(executor: PostgresExecutor) -> HealthModule {
    HealthModule::new("readiness")
        .with_route("/health/ready")
        .indicator("postgres", move || {
            let executor = executor.clone();
            async move { Ok(postgres_health(executor).await) }
        })
}

fn certificate_authority_provider(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn ICertificateAuthority>, ControlPlaneStartupError> {
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
    Ok(certificate_authority)
}

fn recipient_contact_proof_provider(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn IRecipientContactProofService>, ControlPlaneStartupError> {
    let key_id =
        RecipientContactSigningKeyId::parse(&config.security.recipient_contact_proof_key_id)
            .map_err(ControlPlaneStartupError::Security)?;
    match config.security.recipient_contact_proof {
        SecurityProviderKind::Local => Ok(Arc::new(
            HmacRecipientContactProofService::load_or_create(
                key_id,
                std::path::Path::new(&config.security.state_dir)
                    .join("recipient-contact/proof-hmac.key"),
            )
            .map_err(ControlPlaneStartupError::Security)?,
        )),
        SecurityProviderKind::Vault => {
            let (address, token) = credentials.ok_or_else(|| {
                ControlPlaneStartupError::Security("Vault credentials were not resolved".into())
            })?;
            Ok(Arc::new(
                VaultRecipientContactProofService::new(
                    address,
                    token,
                    config.security.vault_transit_mount.clone(),
                    config.security.vault_recipient_contact_proof_key.clone(),
                    key_id,
                    Duration::from_millis(config.security.vault_timeout_ms),
                )
                .map_err(ControlPlaneStartupError::Security)?,
            ))
        }
    }
}

fn key_encryption_provider(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn ISecretEncryptionService>, ControlPlaneStartupError> {
    let timeout = Duration::from_millis(config.security.vault_timeout_ms);
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
    Ok(key_encryption)
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

async fn audit_export_signer(
    config: &CloudConfig,
    credentials: Option<&(String, String)>,
) -> std::result::Result<Arc<dyn IAuditExportSigner>, ControlPlaneStartupError> {
    let signer: Arc<dyn IBuildEvidenceSigner> = match config.security.audit_export_signing {
        SecurityProviderKind::Local => Arc::new(
            LocalBuildEvidenceSigner::load_or_create(
                std::path::Path::new(&config.security.state_dir)
                    .join("audit-export/signing-key.pk8"),
            )
            .await
            .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
        ),
        SecurityProviderKind::Vault => {
            let (address, token) = credentials.ok_or_else(|| {
                ControlPlaneStartupError::Security("Vault credentials were not resolved".into())
            })?;
            Arc::new(
                VaultBuildEvidenceSigner::new(
                    address,
                    token,
                    config.security.vault_transit_mount.clone(),
                    config.security.vault_audit_export_signing_key.clone(),
                    Duration::from_millis(config.security.vault_timeout_ms),
                )
                .map_err(|error| ControlPlaneStartupError::Security(error.to_string()))?,
            )
        }
    };
    Ok(Arc::new(BuildEvidenceAuditExportSigner { signer }))
}

struct BuildEvidenceAuditExportSigner {
    signer: Arc<dyn IBuildEvidenceSigner>,
}

#[async_trait]
impl IAuditExportSigner for BuildEvidenceAuditExportSigner {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> std::result::Result<VerifiedAuditExportSignature, AuditExportSigningError> {
        let signature = self.signer.sign(pae).await.map_err(|error| match error {
            crate::modules::artifacts::BuildEvidenceSigningError::Invalid(message) => {
                AuditExportSigningError::Invalid(message)
            }
            crate::modules::artifacts::BuildEvidenceSigningError::Unavailable(message) => {
                AuditExportSigningError::Unavailable(message)
            }
            crate::modules::artifacts::BuildEvidenceSigningError::Rejected(message) => {
                AuditExportSigningError::Rejected(message)
            }
        })?;
        let key_id = signature
            .key
            .key_id
            .strip_prefix("sha256:")
            .ok_or_else(|| {
                AuditExportSigningError::Rejected(
                    "shared Ed25519 signer returned an incompatible key ID".into(),
                )
            })?
            .to_owned();
        VerifiedAuditExportSignature::new(
            AuditExportSigningKey {
                algorithm: signature.key.algorithm,
                key_id,
                public_key: signature.key.public_key,
                key_version: signature.key.key_version,
            },
            signature.signature,
        )
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

fn object_storage(
    config: &CloudConfig,
) -> std::result::Result<ImmutableObjectClient, ControlPlaneStartupError> {
    match config.objects.provider {
        ObjectStorageProviderKind::Local => {
            ImmutableObjectClient::local(&config.objects.local_dir, &config.objects.prefix)
                .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))
        }
        ObjectStorageProviderKind::S3 => {
            let credentials = config.object_storage_credentials()?.ok_or_else(|| {
                ControlPlaneStartupError::ObjectStorage(
                    "object-storage credentials were not resolved".into(),
                )
            })?;
            ImmutableObjectClient::s3(S3ImmutableObjectOptions {
                endpoint: (!config.objects.endpoint.is_empty())
                    .then(|| config.objects.endpoint.clone()),
                region: config.objects.region.clone(),
                bucket: config.objects.bucket.clone(),
                prefix: config.objects.prefix.clone(),
                access_key_id: credentials.access_key_id,
                secret_access_key: credentials.secret_access_key,
                session_token: credentials.session_token,
                allow_http: config.objects.allow_http,
                virtual_hosted_style: config.objects.virtual_hosted_style,
                request_timeout: Duration::from_millis(config.objects.request_timeout_ms),
                connect_timeout: Duration::from_millis(config.objects.connect_timeout_ms),
                retry_timeout: Duration::from_millis(config.objects.retry_timeout_ms),
                max_retries: config.objects.max_retries,
            })
            .map_err(|error| ControlPlaneStartupError::ObjectStorage(error.to_string()))
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
) -> std::result::Result<Arc<A3sEventPublisher>, ControlPlaneStartupError> {
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
