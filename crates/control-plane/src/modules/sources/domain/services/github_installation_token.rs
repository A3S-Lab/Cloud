use crate::modules::shared_kernel::domain::{OrganizationId, SourceConnectionId};
use crate::modules::sources::domain::{
    GitRepository, GithubInstallationId, SourceProviderCredential,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct GithubInstallationTokenRequest {
    pub organization_id: OrganizationId,
    pub connection_id: SourceConnectionId,
    pub installation_id: GithubInstallationId,
    pub repository: GitRepository,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubInstallationTokenError {
    #[error("GitHub installation tokens are not configured")]
    NotConfigured,
    #[error("GitHub installation cannot access the requested repository")]
    Forbidden,
    #[error("GitHub installation-token provider is unavailable")]
    Unavailable,
    #[error("GitHub installation-token response violated the protocol: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IGithubInstallationTokenService: Send + Sync {
    async fn issue(
        &self,
        request: GithubInstallationTokenRequest,
    ) -> Result<SourceProviderCredential, GithubInstallationTokenError>;
}
