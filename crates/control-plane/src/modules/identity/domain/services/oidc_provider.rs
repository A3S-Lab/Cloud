use crate::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use zeroize::Zeroizing;

pub struct OidcAuthorizationRequest {
    pub provider_key: OidcProviderKey,
    pub state: Zeroizing<String>,
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcAuthorization {
    pub authorization_url: String,
    pub provider_key: OidcProviderKey,
    pub issuer: OidcIssuer,
    pub provider_config_digest: Sha256Digest,
}

pub struct OidcCodeVerificationRequest {
    pub provider_key: OidcProviderKey,
    pub code: Zeroizing<String>,
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOidcIdentity {
    pub provider_key: OidcProviderKey,
    pub issuer: OidcIssuer,
    pub provider_config_digest: Sha256Digest,
    pub subject: ExternalIdentitySubject,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OidcProviderError {
    #[error("OIDC provider is not configured")]
    NotConfigured,
    #[error("OIDC provider credential is unavailable")]
    CredentialUnavailable,
    #[error("OIDC provider is unavailable")]
    Unavailable,
    #[error("OIDC authorization was rejected")]
    Rejected,
    #[error("OIDC provider response violated the protocol: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IOidcProviderService: Send + Sync {
    async fn authorization_url(
        &self,
        request: OidcAuthorizationRequest,
    ) -> Result<OidcAuthorization, OidcProviderError>;

    async fn verify_code(
        &self,
        request: OidcCodeVerificationRequest,
    ) -> Result<VerifiedOidcIdentity, OidcProviderError>;
}
