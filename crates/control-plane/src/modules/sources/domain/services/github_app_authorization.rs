use crate::modules::sources::domain::value_objects::{
    GithubAccountId, GithubAccountKind, GithubInstallationId, GithubLogin,
};
use async_trait::async_trait;
use zeroize::Zeroizing;

pub struct GithubInstallationVerificationRequest {
    pub code: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
    pub installation_id: GithubInstallationId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedGithubInstallation {
    pub installation_id: GithubInstallationId,
    pub account_id: GithubAccountId,
    pub account_login: GithubLogin,
    pub account_kind: GithubAccountKind,
    pub user_id: GithubAccountId,
    pub user_login: GithubLogin,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubAppAuthorizationError {
    #[error("GitHub App connections are not configured")]
    NotConfigured,
    #[error("GitHub authorization was rejected")]
    Rejected,
    #[error("GitHub user cannot access the requested installation")]
    Forbidden,
    #[error("GitHub authorization provider is unavailable")]
    Unavailable,
    #[error("GitHub authorization response violated the protocol: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IGithubAppAuthorizationService: Send + Sync {
    fn installation_url(&self, state: &str) -> Result<String, GithubAppAuthorizationError>;

    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<String, GithubAppAuthorizationError>;

    async fn verify_installation(
        &self,
        request: GithubInstallationVerificationRequest,
    ) -> Result<VerifiedGithubInstallation, GithubAppAuthorizationError>;
}
