use crate::modules::edge::domain::McpCredential;
use crate::modules::secrets::domain::EncryptedSecretValue;
use crate::modules::shared_kernel::domain::{canonical_timestamp, McpCredentialId, OrganizationId};
use chrono::{DateTime, Utc};

/// Short-lived encrypted recovery material for one hosted MCP credential
/// generation.
///
/// The plaintext bearer value is never persisted. A receipt exists only so an
/// idempotent retry can recover the exact value after a commit-before-response
/// failure.
#[derive(Clone, PartialEq, Eq)]
pub struct McpCredentialDeliveryReceipt {
    pub organization_id: OrganizationId,
    pub credential_id: McpCredentialId,
    pub generation: u64,
    pub encrypted_value: EncryptedSecretValue,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl McpCredentialDeliveryReceipt {
    pub fn new(
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
        generation: u64,
        encrypted_value: EncryptedSecretValue,
        expires_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let receipt = Self {
            organization_id,
            credential_id,
            generation,
            encrypted_value,
            expires_at: canonical_timestamp(expires_at),
            created_at: canonical_timestamp(created_at),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate_against(&self, credential: &McpCredential) -> Result<(), String> {
        self.validate()?;
        if self.organization_id != credential.organization_id
            || self.credential_id != credential.id
            || self.generation != credential.generation()
            || self.created_at != credential.updated_at()
            || self.expires_at > credential.expires_at()
            || credential.revoked_at().is_some()
        {
            return Err("MCP credential delivery receipt does not match its generation".into());
        }
        Ok(())
    }

    pub fn is_available_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > canonical_timestamp(now)
    }

    fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.credential_id.as_uuid().is_nil()
            || self.generation == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self.expires_at <= self.created_at
        {
            return Err("MCP credential delivery receipt is invalid".into());
        }
        self.encrypted_value.validate()
    }
}

impl std::fmt::Debug for McpCredentialDeliveryReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpCredentialDeliveryReceipt")
            .field("organization_id", &self.organization_id)
            .field("credential_id", &self.credential_id)
            .field("generation", &self.generation)
            .field("encrypted_value", &self.encrypted_value)
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

pub fn mcp_credential_delivery_context(
    organization_id: OrganizationId,
    credential_id: McpCredentialId,
    generation: u64,
) -> Result<Vec<u8>, String> {
    if organization_id.as_uuid().is_nil() || credential_id.as_uuid().is_nil() || generation == 0 {
        return Err("MCP credential delivery context identity is invalid".into());
    }
    let mut context = Vec::with_capacity(64);
    context.extend_from_slice(b"a3s.cloud.mcp-credential.delivery.v1\0");
    context.extend_from_slice(organization_id.as_uuid().as_bytes());
    context.extend_from_slice(credential_id.as_uuid().as_bytes());
    context.extend_from_slice(&generation.to_be_bytes());
    Ok(context)
}
