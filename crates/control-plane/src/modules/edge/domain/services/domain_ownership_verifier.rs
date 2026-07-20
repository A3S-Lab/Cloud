use crate::modules::edge::domain::DomainNamePattern;
use crate::modules::shared_kernel::domain::DomainClaimId;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainOwnershipVerificationRequest {
    pub claim_id: DomainClaimId,
    pub pattern: DomainNamePattern,
    pub challenge_dns_name: String,
    pub expected_value: String,
    pub presented_proof: String,
}

#[async_trait]
pub trait IDomainOwnershipVerifier: Send + Sync {
    async fn verify(
        &self,
        request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DomainOwnershipVerificationError {
    #[error("domain ownership verification request is invalid: {0}")]
    Invalid(String),
    #[error("domain ownership proof was rejected: {0}")]
    Rejected(String),
    #[error("domain ownership proof is not observable yet: {0}")]
    NotReady(String),
    #[error("domain ownership verifier is unavailable: {0}")]
    Unavailable(String),
}
