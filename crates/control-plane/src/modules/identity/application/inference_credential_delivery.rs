//! One-time inference credential delivery result types.

use crate::modules::identity::domain::entities::InferenceCredential;
use chrono::{DateTime, Utc};
use std::fmt;
use zeroize::Zeroizing;

pub const INFERENCE_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS: i64 = 600;

pub struct InferenceCredentialDeliveryResult {
    pub credential: InferenceCredential,
    pub(crate) bearer_credential: Zeroizing<String>,
    pub delivery_expires_at: DateTime<Utc>,
    pub replayed: bool,
}

impl InferenceCredentialDeliveryResult {
    pub fn bearer_credential(&self) -> &str {
        self.bearer_credential.as_str()
    }

    pub fn into_parts(self) -> (InferenceCredential, Zeroizing<String>, DateTime<Utc>, bool) {
        (
            self.credential,
            self.bearer_credential,
            self.delivery_expires_at,
            self.replayed,
        )
    }
}

impl fmt::Debug for InferenceCredentialDeliveryResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceCredentialDeliveryResult")
            .field("credential", &self.credential)
            .field("bearer_credential", &"<redacted>")
            .field("delivery_expires_at", &self.delivery_expires_at)
            .field("replayed", &self.replayed)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct InferenceCredentialMutationResult {
    pub credential: InferenceCredential,
    pub replayed: bool,
}
