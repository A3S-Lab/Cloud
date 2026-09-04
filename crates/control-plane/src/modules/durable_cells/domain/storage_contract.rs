use crate::modules::shared_kernel::domain::{Sha256Digest, StorageNamespaceId};

/// Identity-only view of an S0 recovery point.
///
/// The Storage owner validates the complete recovery-point aggregate before
/// constructing this value. Durable Cells uses the view only to bind the
/// point to its namespace and provider-profile lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageRecoveryPointIdentity {
    pub namespace_id: StorageNamespaceId,
    pub provider_profile_digest: Sha256Digest,
    pub digest: Sha256Digest,
}

impl DurableCellStorageRecoveryPointIdentity {
    pub fn validate(&self) -> Result<(), String> {
        validate_namespace(self.namespace_id)?;
        validate_digest(&self.provider_profile_digest)?;
        validate_digest(&self.digest)
    }
}

/// Identity-only view of an isolated S0 restore plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageRestorePlanIdentity {
    pub source_namespace_id: StorageNamespaceId,
    pub source_recovery_point_digest: Sha256Digest,
    pub source_provider_profile_digest: Sha256Digest,
    pub target_namespace_id: StorageNamespaceId,
    pub target_provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
    pub digest: Sha256Digest,
}

impl DurableCellStorageRestorePlanIdentity {
    pub fn validate(&self) -> Result<(), String> {
        validate_namespace(self.source_namespace_id)?;
        validate_namespace(self.target_namespace_id)?;
        if self.source_namespace_id == self.target_namespace_id {
            return Err("Durable Cell restore identity must use an isolated namespace".into());
        }
        for digest in [
            &self.source_recovery_point_digest,
            &self.source_provider_profile_digest,
            &self.target_provider_profile_digest,
            &self.retention_policy_digest,
            &self.digest,
        ] {
            validate_digest(digest)?;
        }
        Ok(())
    }
}

/// Identity-only view of provider evidence for an isolated restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageRestoreEvidenceIdentity {
    pub plan_digest: Sha256Digest,
    pub source_recovery_point_digest: Sha256Digest,
    pub target_namespace_id: StorageNamespaceId,
    pub digest: Sha256Digest,
}

impl DurableCellStorageRestoreEvidenceIdentity {
    pub fn validate(&self) -> Result<(), String> {
        validate_namespace(self.target_namespace_id)?;
        for digest in [
            &self.plan_digest,
            &self.source_recovery_point_digest,
            &self.digest,
        ] {
            validate_digest(digest)?;
        }
        Ok(())
    }
}

/// Identity-only view of an S0 deletion plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellStorageDeletionPlanIdentity {
    pub namespace_id: StorageNamespaceId,
    pub provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
    pub latest_recovery_point_digest: Sha256Digest,
    pub verified_restore_evidence_digest: Sha256Digest,
    pub retained_restore_namespace_id: StorageNamespaceId,
    pub digest: Sha256Digest,
}

impl DurableCellStorageDeletionPlanIdentity {
    pub fn validate(&self) -> Result<(), String> {
        validate_namespace(self.namespace_id)?;
        validate_namespace(self.retained_restore_namespace_id)?;
        if self.namespace_id == self.retained_restore_namespace_id {
            return Err("Durable Cell deletion identity must retain an isolated namespace".into());
        }
        for digest in [
            &self.provider_profile_digest,
            &self.retention_policy_digest,
            &self.latest_recovery_point_digest,
            &self.verified_restore_evidence_digest,
            &self.digest,
        ] {
            validate_digest(digest)?;
        }
        Ok(())
    }
}

fn validate_namespace(namespace_id: StorageNamespaceId) -> Result<(), String> {
    if namespace_id.as_uuid().is_nil() {
        return Err("Durable Cell storage identity requires a non-nil namespace".into());
    }
    Ok(())
}

fn validate_digest(digest: &Sha256Digest) -> Result<(), String> {
    if Sha256Digest::parse(digest.as_str())? != *digest {
        return Err("Durable Cell storage identity contains a non-canonical digest".into());
    }
    Ok(())
}
