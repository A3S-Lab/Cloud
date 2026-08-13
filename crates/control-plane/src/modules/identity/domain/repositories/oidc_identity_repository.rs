use crate::modules::identity::domain::entities::{ApiToken, ExternalIdentityLink, OidcFlow};
use crate::modules::identity::domain::value_objects::{
    ApiTokenDigest, ApiTokenName, ExternalIdentitySubject,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, OidcFlowId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CompleteOidcLinkWrite {
    pub flow_id: OidcFlowId,
    pub provider_config_digest: Sha256Digest,
    pub state_digest: Sha256Digest,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier_digest: Sha256Digest,
    pub subject: ExternalIdentitySubject,
    pub completed_at: DateTime<Utc>,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CompleteOidcLoginWrite {
    pub flow_id: OidcFlowId,
    pub provider_config_digest: Sha256Digest,
    pub state_digest: Sha256Digest,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier_digest: Sha256Digest,
    pub subject: ExternalIdentitySubject,
    pub token_id: ApiTokenId,
    pub token_name: ApiTokenName,
    pub token_digest: ApiTokenDigest,
    pub completed_at: DateTime<Utc>,
    pub token_expires_at: DateTime<Utc>,
    pub request_id: Uuid,
}

#[async_trait]
pub trait IOidcIdentityRepository: Send + Sync {
    async fn begin_oidc_flow(&self, flow: OidcFlow) -> Result<OidcFlow, RepositoryError>;

    async fn find_pending_oidc_flow(
        &self,
        state_digest: &Sha256Digest,
        now: DateTime<Utc>,
    ) -> Result<Option<OidcFlow>, RepositoryError>;

    async fn complete_oidc_link(
        &self,
        write: CompleteOidcLinkWrite,
    ) -> Result<ExternalIdentityLink, RepositoryError>;

    async fn complete_oidc_login(
        &self,
        write: CompleteOidcLoginWrite,
    ) -> Result<ApiToken, RepositoryError>;
}
