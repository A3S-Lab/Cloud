use super::{DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellStorageBinding};
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, Sha256Digest};
use a3s_acl::{canonical_digest, generate_acl, parse_acl};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_PROVIDER_PROFILE_ACL_BYTES: usize = 16 * 1024;

/// Immutable correlation intent for projecting one Durable Cell application
/// revision through the existing S0, Workloads, Operations, and Fleet owners.
///
/// This record deliberately has no lifecycle/status field. The referenced
/// Workload Deployment and Operation remain the only execution authorities;
/// persisting this intent before invoking them makes process-death recovery an
/// idempotent replay instead of another rollout controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellDeployment {
    pub projection: DurableCellProjectionIdentity,
    pub storage: DurableCellStorageBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_provider_profile_acl: Option<String>,
    pub provider: DurableCellProviderBinding,
    pub placement_policy_digest: Sha256Digest,
    pub requested_by: PrincipalId,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCellDeploymentRequest {
    pub requested_by: PrincipalId,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellDeployment {
    pub fn bind(
        projection: DurableCellProjectionIdentity,
        storage: DurableCellStorageBinding,
        storage_provider_profile_acl: Option<&str>,
        provider: DurableCellProviderBinding,
        placement_policy_digest: Sha256Digest,
        request: DurableCellDeploymentRequest,
    ) -> Result<Self, String> {
        let deployment = Self {
            projection,
            storage,
            storage_provider_profile_acl: storage_provider_profile_acl.map(str::to_owned),
            provider,
            placement_policy_digest,
            requested_by: request.requested_by,
            request_id: request.request_id,
            requested_at: canonical_timestamp(request.requested_at),
        };
        deployment.validate()?;
        Ok(deployment)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.projection.validate()?;
        self.storage.validate()?;
        if let Some(acl) = &self.storage_provider_profile_acl {
            validate_provider_profile_acl(acl, &self.storage.provider_profile_digest)?;
        }
        self.provider.validate()?;
        if self.storage.organization_id != self.projection.organization_id
            || self.storage.project_id != self.projection.project_id
            || self.storage.environment_id != self.projection.environment_id
            || self.storage.application_id != self.projection.application_id
            || self.storage.application_revision_id != self.projection.application_revision_id
            || self.storage.application_revision_number
                != self.projection.application_revision_number
            || self.storage.application_definition_digest
                != self.projection.application_definition_digest
            || self.storage.storage_namespace_id != self.projection.storage_namespace_id
            || self.provider.application_id != self.projection.application_id
            || self.provider.application_revision_id != self.projection.application_revision_id
            || self.provider.application_revision_number
                != self.projection.application_revision_number
            || self.provider.application_definition_digest
                != self.projection.application_definition_digest
            || self.provider.workload_id != self.projection.workload_id
            || self.provider.workload_revision_id != self.projection.workload_revision_id
            || Sha256Digest::parse(self.placement_policy_digest.as_str())?
                != self.placement_policy_digest
            || self.requested_by.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell deployment correlation is invalid".into());
        }
        Ok(())
    }

    pub fn require_storage_provider_profile_acl(&self) -> Result<&str, String> {
        self.storage_provider_profile_acl()?.ok_or_else(|| {
            "Durable Cell deployment does not bind the optional exact S0 provider profile"
                .to_owned()
        })
    }

    pub fn storage_provider_profile_acl(&self) -> Result<Option<&str>, String> {
        self.validate()?;
        Ok(self.storage_provider_profile_acl.as_deref())
    }
}

/// Validate only the immutable wire identity held by the Durable Cells
/// correlation. Data remains responsible for parsing the provider-specific
/// schema and enforcing endpoint, bucket, and addressing semantics at its
/// Storage adapter.
fn validate_provider_profile_acl(acl: &str, expected_digest: &Sha256Digest) -> Result<(), String> {
    if acl.is_empty() || acl.len() > MAX_PROVIDER_PROFILE_ACL_BYTES || acl.contains('\0') {
        return Err("Durable Cell deployment provider profile ACL size is invalid".into());
    }
    let document = parse_acl(acl).map_err(|error| {
        format!("Durable Cell deployment provider profile ACL is invalid: {error}")
    })?;
    if generate_acl(&document) != acl {
        return Err("Durable Cell deployment provider profile ACL is not canonical".into());
    }
    let digest = Sha256Digest::parse(canonical_digest(&document).map_err(|error| {
        format!("Durable Cell deployment provider profile is not canonicalizable: {error}")
    })?)?;
    if &digest != expected_digest {
        return Err("Durable Cell deployment provider profile ACL and digest do not match".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_acl::builder::{string, BlockBuilder};
    use a3s_acl::Document;

    #[test]
    fn provider_profile_validation_is_opaque_but_digest_locked() {
        let document = Document {
            blocks: vec![BlockBuilder::new("opaque_profile")
                .attr("payload", string("provider-owned"))
                .build()],
        };
        let acl = generate_acl(&document);
        let digest = Sha256Digest::parse(canonical_digest(&document).expect("digest"))
            .expect("canonical digest");

        validate_provider_profile_acl(&acl, &digest).expect("canonical opaque ACL");
        assert!(
            validate_provider_profile_acl(&acl, &Sha256Digest::from_bytes(b"different")).is_err()
        );
        assert!(validate_provider_profile_acl(&format!(" {acl}"), &digest).is_err());
    }
}
