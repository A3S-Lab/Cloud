use crate::modules::edge::application::{
    IEdgeMcpCredentialEncryption, McpCredentialDeliveryResult,
    MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS,
};
use crate::modules::edge::domain::repositories::McpCredentialWrite;
use crate::modules::edge::domain::{
    mcp_credential_delivery_context, EdgeEncryptedCredentialValue, McpCredential,
    McpCredentialDeliveryReceipt,
};
use crate::modules::secrets::application::encryption_error;
use crate::modules::secrets::domain::{EncryptedSecretValue, ISecretEncryptionService};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Consumer-owned anti-corruption adapter for Secrets encryption.
/// It creates no key store, cache, lifecycle, or retry mechanism.
#[derive(Clone)]
pub struct SecretsEdgeMcpCredentialEncryptionAdapter {
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl SecretsEdgeMcpCredentialEncryptionAdapter {
    pub fn new(encryption: Arc<dyn ISecretEncryptionService>) -> Self {
        Self { encryption }
    }
}

#[async_trait]
impl IEdgeMcpCredentialEncryption for SecretsEdgeMcpCredentialEncryptionAdapter {
    async fn encrypt_delivery_receipt(
        &self,
        credential: &McpCredential,
        bearer_credential: &str,
    ) -> ApplicationResult<McpCredentialDeliveryReceipt> {
        validate_bearer(credential, bearer_credential).map_err(ApplicationError::Internal)?;
        let context = mcp_credential_delivery_context(
            credential.organization_id,
            credential.id,
            credential.generation(),
        )
        .map_err(ApplicationError::Internal)?;
        let encrypted = self
            .encryption
            .encrypt(bearer_credential.as_bytes(), &context)
            .await
            .map_err(encryption_error)?;
        let owned = EdgeEncryptedCredentialValue::new(encrypted.key_id(), encrypted.ciphertext())
            .map_err(ApplicationError::Internal)?;
        let delivery_expires_at = std::cmp::min(
            credential.expires_at(),
            credential.updated_at()
                + Duration::seconds(MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS),
        );
        McpCredentialDeliveryReceipt::new(
            credential.organization_id,
            credential.id,
            credential.generation(),
            owned,
            delivery_expires_at,
            credential.updated_at(),
        )
        .map_err(ApplicationError::Internal)
    }

    async fn recover_delivery(
        &self,
        write: McpCredentialWrite,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<McpCredentialDeliveryResult> {
        if write.credential.revoked_at().is_some() {
            return Err(ApplicationError::Conflict(
                "MCP credential was revoked before its delivery could be recovered".into(),
            ));
        }
        let receipt = write.receipt.ok_or_else(|| {
            ApplicationError::Conflict(
                "MCP credential delivery is no longer recoverable; rotate the credential".into(),
            )
        })?;
        if !receipt.is_available_at(observed_at) {
            return Err(ApplicationError::Conflict(
                "MCP credential delivery receipt expired; rotate the credential".into(),
            ));
        }
        receipt
            .validate_against(&write.credential)
            .map_err(ApplicationError::Internal)?;
        let context = mcp_credential_delivery_context(
            write.credential.organization_id,
            write.credential.id,
            write.credential.generation(),
        )
        .map_err(ApplicationError::Internal)?;
        let secrets_value = EncryptedSecretValue::new(
            receipt.encrypted_value.key_id(),
            receipt.encrypted_value.ciphertext(),
        )
        .map_err(ApplicationError::Internal)?;
        let plaintext = Zeroizing::new(
            self.encryption
                .decrypt(&secrets_value, &context)
                .await
                .map_err(encryption_error)?,
        );
        let bearer = std::str::from_utf8(plaintext.as_slice()).map_err(|_| {
            ApplicationError::Internal("decrypted MCP credential delivery is not UTF-8".into())
        })?;
        let bearer = Zeroizing::new(bearer.to_owned());
        validate_bearer(&write.credential, bearer.as_str()).map_err(ApplicationError::Internal)?;
        Ok(McpCredentialDeliveryResult {
            credential: write.credential,
            bearer_credential: bearer,
            delivery_expires_at: receipt.expires_at,
            replayed: write.replayed,
        })
    }
}

fn validate_bearer(credential: &McpCredential, bearer: &str) -> Result<(), String> {
    if bearer.len() != 88
        || !bearer.starts_with(credential.prefix())
        || !bearer
            .bytes()
            .skip(credential.prefix().len())
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("MCP credential delivery does not match its persisted prefix".into());
    }
    Ok(())
}
