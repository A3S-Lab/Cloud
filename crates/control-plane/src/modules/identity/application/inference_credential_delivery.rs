//! One-time inference credential delivery through encrypted receipts.

use crate::modules::identity::domain::entities::{
    inference_credential_delivery_context, InferenceCredential, InferenceCredentialDeliveryReceipt,
};
use crate::modules::identity::domain::repositories::InferenceCredentialWrite;
use crate::modules::secrets::application::encryption_error;
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use zeroize::Zeroizing;

pub const INFERENCE_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS: i64 = 600;

pub struct InferenceCredentialDeliveryResult {
    pub credential: InferenceCredential,
    bearer_credential: Zeroizing<String>,
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

pub async fn encrypt_inference_credential_delivery_receipt(
    encryption: &dyn ISecretEncryptionService,
    credential: &InferenceCredential,
    bearer_credential: &str,
) -> ApplicationResult<InferenceCredentialDeliveryReceipt> {
    validate_bearer(credential, bearer_credential).map_err(ApplicationError::Internal)?;
    let context = inference_credential_delivery_context(
        credential.organization_id,
        credential.id,
        credential.generation(),
    )
    .map_err(ApplicationError::Internal)?;
    let encrypted = encryption
        .encrypt(bearer_credential.as_bytes(), &context)
        .await
        .map_err(encryption_error)?;
    let delivery_expires_at = std::cmp::min(
        credential.expires_at(),
        credential.updated_at()
            + Duration::seconds(INFERENCE_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS),
    );
    InferenceCredentialDeliveryReceipt::new(
        credential.organization_id,
        credential.id,
        credential.generation(),
        encrypted,
        delivery_expires_at,
        credential.updated_at(),
    )
    .map_err(ApplicationError::Internal)
}

pub async fn recover_inference_credential_delivery(
    encryption: &dyn ISecretEncryptionService,
    write: InferenceCredentialWrite,
    observed_at: DateTime<Utc>,
) -> ApplicationResult<InferenceCredentialDeliveryResult> {
    if write.credential.revoked_at().is_some() {
        return Err(ApplicationError::Conflict(
            "inference credential was revoked before its delivery could be recovered".into(),
        ));
    }
    let receipt = write.receipt.ok_or_else(|| {
        ApplicationError::Conflict("inference credential delivery is no longer recoverable".into())
    })?;
    if !receipt.is_available_at(observed_at) {
        return Err(ApplicationError::Conflict(
            "inference credential delivery receipt expired".into(),
        ));
    }
    receipt
        .validate_against(&write.credential)
        .map_err(ApplicationError::Internal)?;
    let context = inference_credential_delivery_context(
        write.credential.organization_id,
        write.credential.id,
        write.credential.generation(),
    )
    .map_err(ApplicationError::Internal)?;
    let plaintext = Zeroizing::new(
        encryption
            .decrypt(&receipt.encrypted_value, &context)
            .await
            .map_err(encryption_error)?,
    );
    let bearer = std::str::from_utf8(plaintext.as_slice()).map_err(|_| {
        ApplicationError::Internal("decrypted inference credential delivery is not UTF-8".into())
    })?;
    let bearer = Zeroizing::new(bearer.to_owned());
    validate_bearer(&write.credential, bearer.as_str()).map_err(ApplicationError::Internal)?;
    Ok(InferenceCredentialDeliveryResult {
        credential: write.credential,
        bearer_credential: bearer,
        delivery_expires_at: receipt.expires_at,
        replayed: write.replayed,
    })
}

fn validate_bearer(credential: &InferenceCredential, bearer: &str) -> Result<(), String> {
    if bearer.len() != 88
        || !bearer.starts_with(credential.prefix())
        || !bearer
            .bytes()
            .skip(credential.prefix().len())
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("inference credential delivery does not match its persisted prefix".into());
    }
    Ok(())
}
