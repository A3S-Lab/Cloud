//! Identity-owned environment inference credentials for Gateway ACL projection.

use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::{
    InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
};
use chrono::{DateTime, Utc};
use std::fmt;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const PREFIX_SUFFIX_LEN: usize = 16;

/// Environment-owned Identity credential for the `cloud-inference` audience.
///
/// Only the stable lookup prefix and Argon2id verifier are retained. The
/// plaintext bearer secret is never part of this aggregate.
#[derive(Clone, PartialEq, Eq)]
pub struct InferenceCredential {
    pub id: InferenceCredentialId,
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

impl InferenceCredential {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: InferenceCredentialId,
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
        id: InferenceCredentialId,
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
            return Err("revoked inference credential cannot be rotated".into());
        }
        let prefix = prefix.into();
        let verifier_hash = verifier_hash.into();
        if prefix == self.prefix || verifier_hash == self.verifier_hash {
            return Err("inference credential rotation must replace its prefix and verifier".into());
        }
        let rotated_at = canonical_timestamp(rotated_at);
        let expires_at = canonical_timestamp(expires_at);
        if rotated_at < self.updated_at || expires_at <= rotated_at {
            return Err("inference credential rotation timestamps are invalid".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference credential generation is exhausted".to_owned())?;
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference credential aggregate version is exhausted".to_owned())?;
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
            return Err("inference credential revocation time regressed".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_ACL_INTEGER)
            .ok_or_else(|| "inference credential aggregate version is exhausted".to_owned())?;
        self.aggregate_version = aggregate_version;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        self.validate()?;
        Ok(true)
    }

    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > canonical_timestamp(now)
    }

    pub fn gateway_projection(&self) -> Result<InferenceCredentialAclProjection, String> {
        InferenceCredentialAclProjection::new(
            self.id.as_uuid(),
            self.environment_id.as_uuid(),
            INFERENCE_CREDENTIAL_AUDIENCE,
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
            return Err("inference credential optimistic transition is invalid".into());
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
            return Err("inference credential update must be one rotation or revocation".into());
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
            return Err("inference credential identity, version, or timestamps are invalid".into());
        }
        if self.revoked_at.is_some_and(|revoked_at| {
            revoked_at != canonical_timestamp(revoked_at)
                || revoked_at < self.created_at
                || revoked_at != self.updated_at
        }) {
            return Err("inference credential revocation timestamp is invalid".into());
        }
        self.gateway_projection()?;
        let Some(suffix) = self.prefix.strip_prefix("a3s_inf_") else {
            return Err("Cloud-issued inference credential prefix must start with a3s_inf_".into());
        };
        if suffix.len() != PREFIX_SUFFIX_LEN
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(
                "Cloud-issued inference credential prefix must use a fixed 16-byte suffix".into(),
            );
        }
        Ok(())
    }
}

impl fmt::Debug for InferenceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceCredential")
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
    use chrono::Duration;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const VERIFIER_ROTATED: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OQ$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

    fn credential() -> InferenceCredential {
        let created_at = Utc::now();
        InferenceCredential::issue(
            InferenceCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap()
    }

    #[test]
    fn projects_cloud_inference_audience_and_redacts_verifier() {
        let credential = credential();
        let projection = credential.gateway_projection().unwrap();
        assert_eq!(projection.audience, INFERENCE_CREDENTIAL_AUDIENCE);
        assert_eq!(projection.prefix, "a3s_inf_0123456789abcdef");
        assert!(!projection.revoked);
        let debug = format!("{credential:?}");
        assert!(!debug.contains(VERIFIER));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn revoke_marks_projection_revoked_and_blocks_rotation() {
        let mut credential = credential();
        let revoked_at = credential.updated_at() + Duration::seconds(1);
        assert!(credential.revoke(revoked_at).unwrap());
        assert!(credential.gateway_projection().unwrap().revoked);
        let err = credential
            .rotate(
                "a3s_inf_fedcba9876543210",
                VERIFIER_ROTATED,
                revoked_at + Duration::hours(1),
                revoked_at + Duration::seconds(1),
            )
            .unwrap_err();
        assert!(err.contains("revoked"));
    }

    #[test]
    fn rejects_mcp_prefix_and_wrong_suffix_length() {
        let created_at = Utc::now();
        let err = InferenceCredential::issue(
            InferenceCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_mcp_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap_err();
        assert!(err.contains("a3s_inf_"));

        let err = InferenceCredential::issue(
            InferenceCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_inf_abc12345",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap_err();
        assert!(err.contains("16-byte"));
    }
}
