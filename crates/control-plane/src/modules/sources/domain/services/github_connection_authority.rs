use crate::modules::shared_kernel::domain::{OrganizationId, SourceConnectionId};
use crate::modules::sources::domain::{
    GithubConnection, GithubInstallationId, GithubProviderAuthority,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct GithubInstallationAuthorityRequest {
    pub installation_id: GithubInstallationId,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubInstallationAuthorityError {
    #[error("GitHub installation authority is not configured")]
    NotConfigured,
    #[error("GitHub installation authority provider is unavailable")]
    Unavailable,
    #[error("GitHub installation authority response violated the protocol: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IGithubInstallationAuthorityProvider: Send + Sync {
    async fn inspect(
        &self,
        request: GithubInstallationAuthorityRequest,
    ) -> Result<GithubProviderAuthority, GithubInstallationAuthorityError>;
}

#[derive(Debug, Clone)]
pub struct GithubConnectionAuthorityRequest {
    pub organization_id: OrganizationId,
    pub connection_id: SourceConnectionId,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubConnectionAuthorityError {
    #[error("GitHub source connection was not found")]
    NotFound,
    #[error("GitHub source connection has no current provider authority")]
    Forbidden,
    #[error("GitHub source connection authority is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait IGithubConnectionAuthorityService: Send + Sync {
    async fn require_current(
        &self,
        request: GithubConnectionAuthorityRequest,
    ) -> Result<GithubConnection, GithubConnectionAuthorityError>;
}
