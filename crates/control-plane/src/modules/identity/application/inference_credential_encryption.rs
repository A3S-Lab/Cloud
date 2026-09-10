use crate::modules::identity::application::InferenceCredentialDeliveryResult;
use crate::modules::identity::domain::entities::{
    InferenceCredential, InferenceCredentialDeliveryReceipt,
};
use crate::modules::identity::domain::repositories::InferenceCredentialWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Identity-owned encryption boundary for inference credential delivery receipts.
///
/// Secrets remains authoritative for key material and ciphertext encoding.
/// Identity Application handlers never import the Secrets encryption service trait.
#[async_trait]
pub trait IIdentityInferenceCredentialEncryption: Send + Sync {
    async fn encrypt_delivery_receipt(
        &self,
        credential: &InferenceCredential,
        bearer_credential: &str,
    ) -> ApplicationResult<InferenceCredentialDeliveryReceipt>;

    async fn recover_delivery(
        &self,
        write: InferenceCredentialWrite,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<InferenceCredentialDeliveryResult>;
}
