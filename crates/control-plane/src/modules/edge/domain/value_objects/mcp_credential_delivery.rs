use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Duration, Utc};
use std::fmt;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_MCP_CREDENTIAL_DELIVERY_TTL: Duration = Duration::hours(1);

/// Short-lived encrypted recovery material for one exact hosted MCP
/// credential generation.
///
/// This value never contains plaintext. Its authenticated encryption context
/// binds the full tenant identity, stable credential, generation, and delivery
/// window so ciphertext cannot be transplanted or have its recovery lifetime
/// extended.
#[derive(Clone, PartialEq, Eq)]
pub struct McpCredentialDelivery {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    credential_id: McpCredentialId,
    generation: u64,
    key_id: String,
    ciphertext: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl McpCredentialDelivery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_id: McpCredentialId,
        generation: u64,
        key_id: impl Into<String>,
        ciphertext: impl Into<String>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let delivery = Self {
            organization_id,
            project_id,
            environment_id,
            credential_id,
            generation,
            key_id: key_id.into(),
            ciphertext: ciphertext.into(),
            created_at,
            expires_at,
        };
        delivery.validate()?;
        Ok(delivery)
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn credential_id(&self) -> McpCredentialId {
        self.credential_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn ciphertext(&self) -> &str {
        &self.ciphertext
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn is_available_at(&self, observed_at: DateTime<Utc>) -> bool {
        canonical_timestamp(observed_at) < self.expires_at
    }

    pub fn matches_credential(&self, credential: &McpCredential) -> bool {
        self.organization_id == credential.organization_id
            && self.project_id == credential.project_id
            && self.environment_id == credential.environment_id
            && self.credential_id == credential.id
            && self.generation == credential.generation()
            && self.created_at == credential.updated_at()
            && self.expires_at <= credential.expires_at()
            && credential.revoked_at().is_none()
    }

    pub fn encryption_context(&self) -> Vec<u8> {
        Self::encryption_context_for(
            self.organization_id,
            self.project_id,
            self.environment_id,
            self.credential_id,
            self.generation,
            self.created_at,
            self.expires_at,
        )
        .expect("validated MCP credential delivery has a valid encryption context")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encryption_context_for(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_id: McpCredentialId,
        generation: u64,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Vec<u8>, String> {
        validate_identity(
            organization_id,
            project_id,
            environment_id,
            credential_id,
            generation,
            created_at,
            expires_at,
        )?;
        let mut context = Vec::with_capacity(113);
        context.extend_from_slice(b"a3s.cloud.mcp-credential-delivery.v1\0");
        context.extend_from_slice(organization_id.as_uuid().as_bytes());
        context.extend_from_slice(project_id.as_uuid().as_bytes());
        context.extend_from_slice(environment_id.as_uuid().as_bytes());
        context.extend_from_slice(credential_id.as_uuid().as_bytes());
        context.extend_from_slice(&generation.to_be_bytes());
        context.extend_from_slice(&created_at.timestamp_micros().to_be_bytes());
        context.extend_from_slice(&expires_at.timestamp_micros().to_be_bytes());
        Ok(context)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_identity(
            self.organization_id,
            self.project_id,
            self.environment_id,
            self.credential_id,
            self.generation,
            self.created_at,
            self.expires_at,
        )?;
        if self.key_id.trim() != self.key_id
            || self.key_id.is_empty()
            || self.key_id.len() > 512
            || self.key_id.contains(['\0', '\r', '\n'])
            || self.ciphertext.trim() != self.ciphertext
            || self.ciphertext.is_empty()
            || self.ciphertext.len() > 2 * 1024 * 1024
            || self.ciphertext.contains(['\0', '\r', '\n'])
        {
            return Err(
                "MCP credential delivery must contain bounded encrypted material and an exact short-lived identity"
                    .into(),
            );
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_identity(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    credential_id: McpCredentialId,
    generation: u64,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<(), String> {
    if organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
        || credential_id.as_uuid().is_nil()
        || generation == 0
        || generation > MAX_SAFE_ACL_INTEGER
        || created_at != canonical_timestamp(created_at)
        || expires_at != canonical_timestamp(expires_at)
        || expires_at <= created_at
        || expires_at - created_at > MAX_MCP_CREDENTIAL_DELIVERY_TTL
    {
        return Err("MCP credential delivery identity or recovery window is invalid".into());
    }
    Ok(())
}

impl fmt::Debug for McpCredentialDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCredentialDelivery")
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .field("environment_id", &self.environment_id)
            .field("credential_id", &self.credential_id)
            .field("generation", &self.generation)
            .field("key_id", &self.key_id)
            .field("ciphertext", &"<redacted-ciphertext>")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn delivery() -> McpCredentialDelivery {
        McpCredentialDelivery::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            McpCredentialId::new(),
            2,
            "local:v1",
            "authenticated-ciphertext",
            now(),
            now() + Duration::minutes(10),
        )
        .expect("delivery")
    }

    #[test]
    fn binds_a_redacted_short_lived_encryption_context() {
        let delivery = delivery();
        assert!(delivery.is_available_at(now()));
        assert!(!delivery.is_available_at(delivery.expires_at()));
        assert!(!format!("{delivery:?}").contains(delivery.ciphertext()));
        assert_ne!(
            delivery.encryption_context(),
            McpCredentialDelivery::new(
                delivery.organization_id(),
                delivery.project_id(),
                delivery.environment_id(),
                delivery.credential_id(),
                delivery.generation() + 1,
                delivery.key_id(),
                delivery.ciphertext(),
                delivery.created_at(),
                delivery.expires_at(),
            )
            .expect("next generation")
            .encryption_context()
        );
    }

    #[test]
    fn rejects_unbounded_or_noncanonical_delivery_material() {
        let valid = delivery();
        assert!(McpCredentialDelivery::new(
            valid.organization_id(),
            valid.project_id(),
            valid.environment_id(),
            valid.credential_id(),
            valid.generation(),
            valid.key_id(),
            valid.ciphertext(),
            valid.created_at(),
            valid.created_at() + MAX_MCP_CREDENTIAL_DELIVERY_TTL + Duration::microseconds(1),
        )
        .is_err());
        assert!(McpCredentialDelivery::new(
            valid.organization_id(),
            valid.project_id(),
            valid.environment_id(),
            valid.credential_id(),
            valid.generation(),
            "bad\nkey",
            valid.ciphertext(),
            valid.created_at(),
            valid.expires_at(),
        )
        .is_err());
    }
}
