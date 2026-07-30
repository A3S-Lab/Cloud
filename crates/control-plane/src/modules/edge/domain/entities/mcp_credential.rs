use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::{McpCredentialProjection, MCP_CREDENTIAL_AUDIENCE};
use chrono::{DateTime, Utc};
use std::fmt;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

/// Cloud-owned service credential for a hosted MCP environment.
///
/// Only the stable lookup prefix and memory-hard verifier are retained. The
/// plaintext bearer credential is never part of this aggregate.
#[derive(Clone, PartialEq, Eq)]
pub struct McpCredential {
    pub id: McpCredentialId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    prefix: String,
    verifier_hash: String,
    generation: u64,
    aggregate_version: u64,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl McpCredential {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: McpCredentialId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        prefix: impl Into<String>,
        verifier_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        Self::restore(
            id,
            organization_id,
            project_id,
            environment_id,
            prefix,
            verifier_hash,
            1,
            1,
            canonical_timestamp(expires_at),
            created_at,
            created_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: McpCredentialId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        prefix: impl Into<String>,
        verifier_hash: impl Into<String>,
        generation: u64,
        aggregate_version: u64,
        expires_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let credential = Self {
            id,
            organization_id,
            project_id,
            environment_id,
            prefix: prefix.into(),
            verifier_hash: verifier_hash.into(),
            generation,
            aggregate_version,
            expires_at,
            created_at,
            updated_at,
            revoked_at,
        };
        credential.validate()?;
        Ok(credential)
    }

    pub fn rotate(
        &mut self,
        prefix: impl Into<String>,
        verifier_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
        rotated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.revoked_at.is_some() {
            return Err("revoked MCP credential cannot be rotated".into());
        }
        let prefix = prefix.into();
        let verifier_hash = verifier_hash.into();
        if prefix == self.prefix || verifier_hash == self.verifier_hash {
            return Err("MCP credential rotation must replace its prefix and verifier".into());
        }
        let rotated_at = canonical_timestamp(rotated_at);
        let expires_at = canonical_timestamp(expires_at);
        if rotated_at < self.updated_at || expires_at <= rotated_at {
            return Err("MCP credential rotation timestamps are invalid".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "MCP credential generation is exhausted".to_owned())?;
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "MCP credential aggregate version is exhausted".to_owned())?;
        let candidate = Self::restore(
            self.id,
            self.organization_id,
            self.project_id,
            self.environment_id,
            prefix,
            verifier_hash,
            generation,
            aggregate_version,
            expires_at,
            self.created_at,
            rotated_at,
            None,
        )?;
        *self = candidate;
        Ok(())
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> Result<bool, String> {
        if self.revoked_at.is_some() {
            return Ok(false);
        }
        let revoked_at = canonical_timestamp(revoked_at);
        if revoked_at < self.updated_at {
            return Err("MCP credential revocation time regressed".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "MCP credential aggregate version is exhausted".to_owned())?;
        self.aggregate_version = aggregate_version;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        self.validate()?;
        Ok(true)
    }

    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > canonical_timestamp(now)
    }

    pub fn gateway_projection(&self) -> McpCredentialProjection {
        McpCredentialProjection::new(
            self.id.as_uuid(),
            self.environment_id.as_uuid(),
            MCP_CREDENTIAL_AUDIENCE,
            &self.prefix,
            &self.verifier_hash,
            self.generation,
            self.expires_at,
            self.revoked_at.is_some(),
        )
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn aggregate_version(&self) -> u64 {
        self.aggregate_version
    }

    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub const fn revoked_at(&self) -> Option<DateTime<Utc>> {
        self.revoked_at
    }

    pub(crate) fn validate_transition_from(
        &self,
        existing: &Self,
        expected_aggregate_version: u64,
    ) -> Result<(), String> {
        if existing.aggregate_version != expected_aggregate_version
            || self.aggregate_version != expected_aggregate_version.checked_add(1).unwrap_or(0)
            || self.id != existing.id
            || self.organization_id != existing.organization_id
            || self.project_id != existing.project_id
            || self.environment_id != existing.environment_id
            || self.created_at != existing.created_at
            || self.updated_at < existing.updated_at
            || existing.revoked_at.is_some()
        {
            return Err("MCP credential optimistic transition is invalid".into());
        }
        let rotated = self.generation == existing.generation.checked_add(1).unwrap_or(0)
            && self.revoked_at.is_none()
            && self.prefix != existing.prefix
            && self.verifier_hash != existing.verifier_hash;
        let revoked = self.generation == existing.generation
            && self.revoked_at == Some(self.updated_at)
            && self.prefix == existing.prefix
            && self.verifier_hash == existing.verifier_hash
            && self.expires_at == existing.expires_at;
        if !rotated && !revoked {
            return Err("MCP credential update must be one rotation or revocation".into());
        }
        self.validate()
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.generation == 0
            || self.generation > MAX_SAFE_ACL_INTEGER
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_SAFE_ACL_INTEGER
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self.updated_at < self.created_at
            || self.expires_at <= self.created_at
            || self.revoked_at.is_none() && self.expires_at <= self.updated_at
        {
            return Err("MCP credential identity, version, or timestamps are invalid".into());
        }
        if self.revoked_at.is_some_and(|revoked_at| {
            revoked_at != canonical_timestamp(revoked_at)
                || revoked_at < self.created_at
                || revoked_at != self.updated_at
        }) {
            return Err("MCP credential revocation timestamp is invalid".into());
        }
        self.gateway_projection().validate()?;
        if self
            .prefix
            .strip_prefix("a3s_mcp_")
            .is_none_or(|suffix| suffix.len() != 16)
        {
            return Err(
                "Cloud-issued MCP credential prefix must use a fixed 16-byte suffix".into(),
            );
        }
        Ok(())
    }
}

impl fmt::Debug for McpCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCredential")
            .field("id", &self.id)
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .field("environment_id", &self.environment_id)
            .field("prefix", &self.prefix)
            .field("verifier_hash", &"<redacted>")
            .field("generation", &self.generation)
            .field("aggregate_version", &self.aggregate_version)
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn credential() -> McpCredential {
        McpCredential::issue(
            McpCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::days(30),
            now(),
        )
        .expect("credential")
    }

    #[test]
    fn issues_only_a_redacted_gateway_compatible_verifier() {
        let credential = credential();
        let projection = credential.gateway_projection();

        assert_eq!(credential.generation(), 1);
        assert_eq!(credential.aggregate_version(), 1);
        assert!(credential.is_active_at(now()));
        assert_eq!(projection.audience, MCP_CREDENTIAL_AUDIENCE);
        assert_eq!(projection.prefix, "a3s_mcp_abc12345def67890");
        assert_eq!(projection.verifier_hash(), VERIFIER);
        assert!(!format!("{credential:?}").contains(VERIFIER));
        assert!(!format!("{projection:?}").contains(VERIFIER));
    }

    #[test]
    fn rotation_advances_generation_and_revocation_is_terminal() {
        let mut credential = credential();
        credential
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        assert_eq!(credential.generation(), 2);
        assert_eq!(credential.aggregate_version(), 2);
        assert_eq!(credential.prefix(), "a3s_mcp_def67890abc12345");
        assert_eq!(
            credential.gateway_projection().verifier_hash(),
            ROTATED_VERIFIER
        );

        assert!(credential
            .revoke(now() + Duration::minutes(2))
            .expect("revoke"));
        assert!(!credential
            .revoke(now() + Duration::minutes(3))
            .expect("idempotent revoke"));
        assert!(!credential.is_active_at(now() + Duration::minutes(2)));
        assert!(credential.gateway_projection().revoked);
        assert!(credential
            .rotate(
                "a3s_mcp_ghi12345jkl67890",
                VERIFIER,
                now() + Duration::days(90),
                now() + Duration::minutes(3),
            )
            .is_err());
    }

    #[test]
    fn rejects_invalid_prefix_verifier_scope_and_timestamps() {
        let valid = credential();
        assert!(McpCredential::issue(
            McpCredentialId::new(),
            valid.organization_id,
            valid.project_id,
            valid.environment_id,
            "a3s_bad_abc12345def67890",
            VERIFIER,
            now() + Duration::days(1),
            now(),
        )
        .is_err());
        assert!(McpCredential::issue(
            McpCredentialId::new(),
            valid.organization_id,
            valid.project_id,
            valid.environment_id,
            "a3s_mcp_abc12345",
            VERIFIER,
            now() + Duration::days(1),
            now(),
        )
        .is_err());
        assert!(McpCredential::issue(
            McpCredentialId::new(),
            valid.organization_id,
            valid.project_id,
            valid.environment_id,
            "a3s_mcp_abc12345def67890",
            "sha256:not-a-verifier",
            now() + Duration::days(1),
            now(),
        )
        .is_err());
        assert!(McpCredential::issue(
            McpCredentialId::from_uuid(uuid::Uuid::nil()),
            valid.organization_id,
            valid.project_id,
            valid.environment_id,
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::days(1),
            now(),
        )
        .is_err());
        assert!(McpCredential::issue(
            McpCredentialId::new(),
            valid.organization_id,
            valid.project_id,
            valid.environment_id,
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now(),
            now(),
        )
        .is_err());
    }
}
