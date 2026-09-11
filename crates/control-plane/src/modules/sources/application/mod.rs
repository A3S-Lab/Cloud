mod authorized_source_checkout;
pub mod commands;
mod github_connection_authority_reconciler;
pub(crate) mod github_flow_security;
mod github_source_discovery;
mod owner_scope_access;
mod preview_source_revision_projection;
pub mod queries;
mod resource_access;
mod source_build_input;
mod source_repository_credential;

pub use authorized_source_checkout::{AuthorizedSourceCheckoutService, IAuthorizedSourceCheckout};
pub use github_connection_authority_reconciler::{
    GithubConnectionAuthorityReconcileReport, GithubConnectionAuthorityReconciler,
};
pub use github_source_discovery::{
    DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE, GITHUB_SOURCE_DISCOVERY_CURSOR_PATTERN,
    GithubDiscoveredReference, GithubDiscoveredReferenceKind, GithubDiscoveredRepository,
    GithubRepositoryDiscoveryPage, GithubRepositoryDiscoveryProviderRequest,
    GithubRepositoryReferenceDiscoveryPage, GithubRepositoryReferenceDiscoveryProviderRequest,
    GithubSourceDiscoveryProviderError, GithubSourceDiscoveryProviderPage,
    GithubSourceDiscoveryQueryService, GithubSourceDiscoveryScope, IGithubSourceDiscoveryProvider,
    MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES, MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
};
pub use owner_scope_access::{ISourceEnvironmentAccess, ISourceOrganizationAccess};
pub(in crate::modules::sources) use preview_source_revision_projection::lifecycle_event;
pub use preview_source_revision_projection::{
    IPreviewSourceRevisionProjectionPort, PreviewSourceRevisionDesiredState,
    PreviewSourceRevisionProjectionOutcome, PreviewSourceRevisionProjectionReceipt,
    ProjectPreviewSourceRevision,
};
pub use resource_access::SourceAccess;
pub(crate) use resource_access::SourceAccessScope;
#[cfg(test)]
pub(crate) use source_build_input::publish_source_build_input;
pub use source_build_input::{
    ISourceBuildInputQueryPort, SourceBuildInputQueryError, SourceBuildInputQueryService,
};
pub use source_repository_credential::{
    ISourceRepositoryCredentialProvider, SourceRepositoryCredentialError,
    SourceRepositoryCredentialRequest, SourceRepositoryCredentialService,
};
