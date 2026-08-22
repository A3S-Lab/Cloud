use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationClaims,
};
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecipientContactProofError {
    #[error("recipient contact verification proof was rejected")]
    Rejected,
    #[error("recipient contact verification proof service is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait IRecipientContactProofService: Send + Sync {
    fn current_key_id(&self) -> &RecipientContactSigningKeyId;

    async fn issue(
        &self,
        verification: &RecipientContactVerification,
    ) -> Result<Zeroizing<String>, RecipientContactProofError>;

    async fn verify(
        &self,
        proof: &str,
        now: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationClaims, RecipientContactProofError>;
}
