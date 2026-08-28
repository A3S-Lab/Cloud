pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod published;

#[cfg(test)]
pub(crate) use application::publish_source_build_input;
pub use application::{
    AuthorizedSourceCheckoutService, GithubConnectionAuthorityReconcileReport,
    GithubConnectionAuthorityReconciler, GithubDiscoveredReference, GithubDiscoveredReferenceKind,
    GithubDiscoveredRepository, GithubRepositoryDiscoveryPage,
    GithubRepositoryDiscoveryProviderRequest, GithubRepositoryReferenceDiscoveryPage,
    GithubRepositoryReferenceDiscoveryProviderRequest, GithubSourceDiscoveryProviderError,
    GithubSourceDiscoveryProviderPage, GithubSourceDiscoveryQueryService,
    GithubSourceDiscoveryScope, IAuthorizedSourceCheckout, IGithubSourceDiscoveryProvider,
    IPreviewSourceRevisionProjectionPort, ISourceBuildInputQueryPort,
    ISourceRepositoryCredentialProvider, PreviewSourceRevisionDesiredState,
    PreviewSourceRevisionProjectionOutcome, PreviewSourceRevisionProjectionReceipt,
    ProjectPreviewSourceRevision, SourceBuildInputQueryError, SourceBuildInputQueryService,
    SourceRepositoryCredentialError, SourceRepositoryCredentialRequest,
    SourceRepositoryCredentialService, DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
    GITHUB_SOURCE_DISCOVERY_CURSOR_PATTERN, MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES,
    MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
};

pub use application::commands::accept_source_webhook_delivery::{
    AcceptSourceWebhookDelivery, AcceptSourceWebhookDeliveryHandler,
    AcceptSourceWebhookDeliveryResult,
};
pub use application::commands::begin_github_connection::{
    BeginGithubConnection, BeginGithubConnectionHandler, BeginGithubConnectionResult,
};
pub use application::commands::complete_github_connection::{
    CompleteGithubConnection, CompleteGithubConnectionHandler,
};
pub use application::commands::create_github_repository_subscription::{
    CreateGithubRepositorySubscription, CreateGithubRepositorySubscriptionHandler,
    CreateGithubRepositorySubscriptionResult,
};
pub use application::commands::deactivate_github_repository_subscription::{
    DeactivateGithubRepositorySubscription, DeactivateGithubRepositorySubscriptionHandler,
    DeactivateGithubRepositorySubscriptionResult,
};
pub use application::commands::prepare_github_connection_oauth::{
    PrepareGithubConnectionOauth, PrepareGithubConnectionOauthHandler,
    PrepareGithubConnectionOauthResult,
};
pub use application::commands::reconcile_github_connection_lifecycle::{
    ReconcileGithubConnectionLifecycle, ReconcileGithubConnectionLifecycleHandler,
};
pub use application::commands::resolve_external_source_revision::{
    DockerfileBuildRecipeInput, ResolveExternalSourceRevision,
    ResolveExternalSourceRevisionHandler, ResolveExternalSourceRevisionResult,
};
pub use application::queries::get_github_connection::{
    GetGithubConnection, GetGithubConnectionHandler,
};
pub use application::queries::github_source_discovery::{
    ListGithubInstallationRepositories, ListGithubInstallationRepositoriesHandler,
    ListGithubRepositoryReferences, ListGithubRepositoryReferencesHandler,
};
pub use application::queries::list_github_repository_subscriptions::{
    ListGithubRepositorySubscriptions, ListGithubRepositorySubscriptionsHandler,
};
pub use application::queries::list_source_revisions::{
    ListSourceRevisions, ListSourceRevisionsHandler,
};
pub use infrastructure::persistence::{
    InMemoryGithubConnectionRepository, InMemorySourceRevisionRepository,
    PostgresGithubConnectionRepository, PostgresSourceRevisionRepository,
    PostgresSourceSubscriptionRepository,
};
pub use infrastructure::{
    DeveloperWorkflowSourceLayoutAdapter, ExternalSourceBuildArchiveAdapter, GitSourceCheckout,
    GithubAppClient, GithubInstallationTokenIssuer, GithubSourceResolver, GithubWebhookVerifier,
    PullRequestPreviewSourceProjector, RevalidatingGithubInstallationTokens,
    RevalidatingGithubSourceDiscovery,
};
pub use presentation::SourcesModule;
pub(crate) use presentation::{
    GithubRepositoryDiscoveryPageResponse, GithubRepositoryReferenceDiscoveryPageResponse,
    GITHUB_REPOSITORY_DISCOVERY_ROUTE, GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
    GITHUB_SOURCE_CONNECTION_ROUTE, SOURCES_CONTROLLER_PREFIX,
};
