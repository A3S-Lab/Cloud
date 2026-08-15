use super::{ObjectNamespaceKey, ObjectNamespaceRetentionPolicy};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, Sha256Digest, StorageNamespaceId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_RECOVERY_CONTRACT_BYTES: usize = 32 * 1024;
const MAX_SAFE_SERIALIZED_INTEGER: u64 = 9_007_199_254_740_991;

/// One immutable, provider-sealed cut of an S0 namespace.
///
/// The manifest and state digests identify provider-owned bytes. S0 owns this
/// lineage contract; consumers must not mirror the underlying state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRecoveryPointSpec {
    pub namespace_id: StorageNamespaceId,
    pub sequence: u64,
    pub writer_epoch: u64,
    pub provider_profile_digest: Sha256Digest,
    pub manifest_key: ObjectNamespaceKey,
    pub manifest_digest: Sha256Digest,
    pub state_digest: Sha256Digest,
    pub state_size_bytes: u64,
    pub predecessor_digest: Option<Sha256Digest>,
    pub sealed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRecoveryPoint {
    spec: ObjectNamespaceRecoveryPointSpec,
    digest: Sha256Digest,
}

impl ObjectNamespaceRecoveryPoint {
    pub fn seal(mut spec: ObjectNamespaceRecoveryPointSpec) -> Result<Self, String> {
        spec.sealed_at = canonical_timestamp(spec.sealed_at);
        validate_recovery_point_spec(&spec)?;
        let digest = contract_digest(&spec, "object namespace recovery point")?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: ObjectNamespaceRecoveryPointSpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        if spec.sealed_at != canonical_timestamp(spec.sealed_at) {
            return Err("stored object namespace recovery point timestamp is not canonical".into());
        }
        let point = Self::seal(spec)?;
        if point.digest.as_str() != stored_digest {
            return Err("stored object namespace recovery point digest drifted".into());
        }
        Ok(point)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::restore(self.spec.clone(), self.digest.as_str())? != self {
            return Err("object namespace recovery point drifted".into());
        }
        Ok(())
    }

    pub fn validate_successor_of(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        previous.validate()?;
        if self.spec.namespace_id != previous.spec.namespace_id
            || self.spec.sequence
                != previous
                    .spec
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| "object namespace recovery sequence is exhausted".to_owned())?
            || self.spec.predecessor_digest.as_ref() != Some(previous.digest())
            || self.spec.writer_epoch < previous.spec.writer_epoch
            || self.spec.sealed_at < previous.spec.sealed_at
        {
            return Err(
                "object namespace recovery successor does not extend the exact sealed lineage"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn spec(&self) -> &ObjectNamespaceRecoveryPointSpec {
        &self.spec
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_recovery_point_spec(spec: &ObjectNamespaceRecoveryPointSpec) -> Result<(), String> {
    let predecessor_shape_is_valid = match (&spec.predecessor_digest, spec.sequence) {
        (None, 1) => true,
        (Some(digest), sequence) if sequence > 1 => {
            Sha256Digest::parse(digest.as_str())? == *digest
        }
        _ => false,
    };
    if spec.namespace_id.as_uuid().is_nil()
        || spec.sequence == 0
        || spec.sequence > MAX_SAFE_SERIALIZED_INTEGER
        || spec.writer_epoch == 0
        || spec.writer_epoch > MAX_SAFE_SERIALIZED_INTEGER
        || spec.state_size_bytes == 0
        || spec.state_size_bytes > MAX_SAFE_SERIALIZED_INTEGER
        || spec.sealed_at != canonical_timestamp(spec.sealed_at)
        || !predecessor_shape_is_valid
        || Sha256Digest::parse(spec.provider_profile_digest.as_str())?
            != spec.provider_profile_digest
        || Sha256Digest::parse(spec.manifest_digest.as_str())? != spec.manifest_digest
        || Sha256Digest::parse(spec.state_digest.as_str())? != spec.state_digest
    {
        return Err("object namespace recovery point is invalid".into());
    }
    ObjectNamespaceKey::parse(spec.manifest_key.as_str().to_owned())?;
    Ok(())
}

/// Exact, isolated restore intent. Restoring in place is deliberately invalid;
/// cutover remains an Operation/Flow responsibility after verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRestorePlanSpec {
    pub source_namespace_id: StorageNamespaceId,
    pub source_recovery_point_digest: Sha256Digest,
    pub source_provider_profile_digest: Sha256Digest,
    pub source_manifest_key: ObjectNamespaceKey,
    pub source_manifest_digest: Sha256Digest,
    pub source_state_digest: Sha256Digest,
    pub source_state_size_bytes: u64,
    pub source_sealed_at: DateTime<Utc>,
    pub target_namespace_id: StorageNamespaceId,
    pub target_provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRestorePlan {
    spec: ObjectNamespaceRestorePlanSpec,
    digest: Sha256Digest,
}

impl ObjectNamespaceRestorePlan {
    pub fn for_recovery_point(
        point: &ObjectNamespaceRecoveryPoint,
        target_namespace_id: StorageNamespaceId,
        target_provider_profile_digest: Sha256Digest,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        point.validate()?;
        retention_policy.validate()?;
        let source = point.spec();
        let spec = ObjectNamespaceRestorePlanSpec {
            source_namespace_id: source.namespace_id,
            source_recovery_point_digest: point.digest().clone(),
            source_provider_profile_digest: source.provider_profile_digest.clone(),
            source_manifest_key: source.manifest_key.clone(),
            source_manifest_digest: source.manifest_digest.clone(),
            source_state_digest: source.state_digest.clone(),
            source_state_size_bytes: source.state_size_bytes,
            source_sealed_at: source.sealed_at,
            target_namespace_id,
            target_provider_profile_digest,
            retention_policy_digest: retention_policy.digest().clone(),
            requested_at: canonical_timestamp(requested_at),
        };
        Self::from_spec(spec)
    }

    fn from_spec(spec: ObjectNamespaceRestorePlanSpec) -> Result<Self, String> {
        validate_restore_plan_spec(&spec)?;
        let digest = contract_digest(&spec, "object namespace restore plan")?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: ObjectNamespaceRestorePlanSpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        if spec.source_sealed_at != canonical_timestamp(spec.source_sealed_at)
            || spec.requested_at != canonical_timestamp(spec.requested_at)
        {
            return Err("stored object namespace restore plan timestamp is not canonical".into());
        }
        let plan = Self::from_spec(spec)?;
        if plan.digest.as_str() != stored_digest {
            return Err("stored object namespace restore plan digest drifted".into());
        }
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::restore(self.spec.clone(), self.digest.as_str())? != self {
            return Err("object namespace restore plan drifted".into());
        }
        Ok(())
    }

    pub fn validate_source(
        &self,
        point: &ObjectNamespaceRecoveryPoint,
        retention_policy: &ObjectNamespaceRetentionPolicy,
    ) -> Result<(), String> {
        self.validate()?;
        point.validate()?;
        retention_policy.validate()?;
        let expected = Self::for_recovery_point(
            point,
            self.spec.target_namespace_id,
            self.spec.target_provider_profile_digest.clone(),
            retention_policy,
            self.spec.requested_at,
        )?;
        if &expected != self {
            return Err(
                "object namespace restore plan does not match its exact sealed source".into(),
            );
        }
        Ok(())
    }

    pub fn spec(&self) -> &ObjectNamespaceRestorePlanSpec {
        &self.spec
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_restore_plan_spec(spec: &ObjectNamespaceRestorePlanSpec) -> Result<(), String> {
    if spec.source_namespace_id.as_uuid().is_nil()
        || spec.target_namespace_id.as_uuid().is_nil()
        || spec.source_namespace_id == spec.target_namespace_id
        || spec.source_state_size_bytes == 0
        || spec.source_state_size_bytes > MAX_SAFE_SERIALIZED_INTEGER
        || spec.source_sealed_at != canonical_timestamp(spec.source_sealed_at)
        || spec.requested_at != canonical_timestamp(spec.requested_at)
        || spec.requested_at < spec.source_sealed_at
        || Sha256Digest::parse(spec.source_recovery_point_digest.as_str())?
            != spec.source_recovery_point_digest
        || Sha256Digest::parse(spec.source_provider_profile_digest.as_str())?
            != spec.source_provider_profile_digest
        || Sha256Digest::parse(spec.source_manifest_digest.as_str())? != spec.source_manifest_digest
        || Sha256Digest::parse(spec.source_state_digest.as_str())? != spec.source_state_digest
        || Sha256Digest::parse(spec.target_provider_profile_digest.as_str())?
            != spec.target_provider_profile_digest
        || Sha256Digest::parse(spec.retention_policy_digest.as_str())?
            != spec.retention_policy_digest
    {
        return Err("object namespace restore plan is invalid or not isolated".into());
    }
    ObjectNamespaceKey::parse(spec.source_manifest_key.as_str().to_owned())?;
    Ok(())
}

/// Provider receipt for an exact isolated restore. The source manifest digest
/// is observed again after restore so an implementation cannot claim success
/// after mutating the sealed source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRestoreEvidence {
    pub plan_digest: Sha256Digest,
    pub source_recovery_point_digest: Sha256Digest,
    pub source_post_restore_manifest_digest: Sha256Digest,
    pub target_namespace_id: StorageNamespaceId,
    pub restored_state_digest: Sha256Digest,
    pub restored_state_size_bytes: u64,
    pub provider_receipt_digest: Sha256Digest,
    pub verified_at: DateTime<Utc>,
    digest: Sha256Digest,
}

impl ObjectNamespaceRestoreEvidence {
    pub fn verified(
        plan: &ObjectNamespaceRestorePlan,
        provider_receipt_digest: Sha256Digest,
        verified_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        plan.validate()?;
        Sha256Digest::parse(provider_receipt_digest.as_str())?;
        let spec = plan.spec();
        let mut evidence = Self {
            plan_digest: plan.digest().clone(),
            source_recovery_point_digest: spec.source_recovery_point_digest.clone(),
            source_post_restore_manifest_digest: spec.source_manifest_digest.clone(),
            target_namespace_id: spec.target_namespace_id,
            restored_state_digest: spec.source_state_digest.clone(),
            restored_state_size_bytes: spec.source_state_size_bytes,
            provider_receipt_digest,
            verified_at: canonical_timestamp(verified_at),
            digest: Sha256Digest::from_bytes(b"uninitialized"),
        };
        validate_restore_evidence_shape(&evidence)?;
        evidence.digest = restore_evidence_digest(&evidence)?;
        evidence.validate_for(plan)?;
        Ok(evidence)
    }

    pub fn validate_for(&self, plan: &ObjectNamespaceRestorePlan) -> Result<(), String> {
        plan.validate()?;
        validate_restore_evidence_shape(self)?;
        if self.digest != restore_evidence_digest(self)?
            || self.plan_digest != *plan.digest()
            || self.source_recovery_point_digest != plan.spec.source_recovery_point_digest
            || self.source_post_restore_manifest_digest != plan.spec.source_manifest_digest
            || self.target_namespace_id != plan.spec.target_namespace_id
            || self.restored_state_digest != plan.spec.source_state_digest
            || self.restored_state_size_bytes != plan.spec.source_state_size_bytes
            || self.verified_at < plan.spec.requested_at
        {
            return Err("object namespace restore evidence does not prove the exact plan".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_restore_evidence_shape(
    evidence: &ObjectNamespaceRestoreEvidence,
) -> Result<(), String> {
    if evidence.target_namespace_id.as_uuid().is_nil()
        || evidence.restored_state_size_bytes == 0
        || evidence.restored_state_size_bytes > MAX_SAFE_SERIALIZED_INTEGER
        || evidence.verified_at != canonical_timestamp(evidence.verified_at)
        || Sha256Digest::parse(evidence.plan_digest.as_str())? != evidence.plan_digest
        || Sha256Digest::parse(evidence.source_recovery_point_digest.as_str())?
            != evidence.source_recovery_point_digest
        || Sha256Digest::parse(evidence.source_post_restore_manifest_digest.as_str())?
            != evidence.source_post_restore_manifest_digest
        || Sha256Digest::parse(evidence.restored_state_digest.as_str())?
            != evidence.restored_state_digest
        || Sha256Digest::parse(evidence.provider_receipt_digest.as_str())?
            != evidence.provider_receipt_digest
    {
        return Err("object namespace restore evidence is invalid".into());
    }
    Ok(())
}

/// Namespace deletion is admitted only after exact writer fencing, retention
/// disposition, a verified isolated restore, and the policy's grace period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceDeletionPlanSpec {
    pub namespace_id: StorageNamespaceId,
    pub provider_profile_digest: Sha256Digest,
    pub retention_policy_digest: Sha256Digest,
    pub latest_recovery_point_digest: Sha256Digest,
    pub verified_restore_evidence_digest: Sha256Digest,
    pub retained_restore_namespace_id: StorageNamespaceId,
    pub writer_fence_receipt_digest: Sha256Digest,
    pub retention_disposition_receipt_digest: Sha256Digest,
    pub requested_at: DateTime<Utc>,
    pub not_before: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceDeletionPlan {
    spec: ObjectNamespaceDeletionPlanSpec,
    digest: Sha256Digest,
}

impl ObjectNamespaceDeletionPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn after_verified_restore(
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        writer_fence_receipt_digest: Sha256Digest,
        retention_disposition_receipt_digest: Sha256Digest,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        restore_plan.validate_source(point, retention_policy)?;
        restore_evidence.validate_for(restore_plan)?;
        Sha256Digest::parse(writer_fence_receipt_digest.as_str())?;
        Sha256Digest::parse(retention_disposition_receipt_digest.as_str())?;
        let requested_at = canonical_timestamp(requested_at);
        if requested_at < restore_evidence.verified_at {
            return Err("object namespace deletion predates isolated restore verification".into());
        }
        let spec = ObjectNamespaceDeletionPlanSpec {
            namespace_id: point.spec.namespace_id,
            provider_profile_digest: point.spec.provider_profile_digest.clone(),
            retention_policy_digest: retention_policy.digest().clone(),
            latest_recovery_point_digest: point.digest().clone(),
            verified_restore_evidence_digest: restore_evidence.digest().clone(),
            retained_restore_namespace_id: restore_plan.spec.target_namespace_id,
            writer_fence_receipt_digest,
            retention_disposition_receipt_digest,
            requested_at,
            not_before: canonical_timestamp(retention_policy.deletion_not_before(requested_at)?),
        };
        Self::from_spec(spec)
    }

    fn from_spec(spec: ObjectNamespaceDeletionPlanSpec) -> Result<Self, String> {
        validate_deletion_plan_spec(&spec)?;
        let digest = contract_digest(&spec, "object namespace deletion plan")?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: ObjectNamespaceDeletionPlanSpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        if spec.requested_at != canonical_timestamp(spec.requested_at)
            || spec.not_before != canonical_timestamp(spec.not_before)
        {
            return Err("stored object namespace deletion plan timestamp is not canonical".into());
        }
        let plan = Self::from_spec(spec)?;
        if plan.digest.as_str() != stored_digest {
            return Err("stored object namespace deletion plan digest drifted".into());
        }
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::restore(self.spec.clone(), self.digest.as_str())? != self {
            return Err("object namespace deletion plan drifted".into());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        retention_policy: &ObjectNamespaceRetentionPolicy,
    ) -> Result<(), String> {
        self.validate()?;
        let expected = Self::after_verified_restore(
            point,
            restore_plan,
            restore_evidence,
            retention_policy,
            self.spec.writer_fence_receipt_digest.clone(),
            self.spec.retention_disposition_receipt_digest.clone(),
            self.spec.requested_at,
        )?;
        if &expected != self {
            return Err("object namespace deletion plan changed supporting evidence".into());
        }
        Ok(())
    }

    pub fn spec(&self) -> &ObjectNamespaceDeletionPlanSpec {
        &self.spec
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_deletion_plan_spec(spec: &ObjectNamespaceDeletionPlanSpec) -> Result<(), String> {
    if spec.namespace_id.as_uuid().is_nil()
        || spec.retained_restore_namespace_id.as_uuid().is_nil()
        || spec.namespace_id == spec.retained_restore_namespace_id
        || spec.requested_at != canonical_timestamp(spec.requested_at)
        || spec.not_before != canonical_timestamp(spec.not_before)
        || spec.not_before <= spec.requested_at
    {
        return Err("object namespace deletion plan is invalid or not isolated".into());
    }
    for digest in [
        &spec.provider_profile_digest,
        &spec.retention_policy_digest,
        &spec.latest_recovery_point_digest,
        &spec.verified_restore_evidence_digest,
        &spec.writer_fence_receipt_digest,
        &spec.retention_disposition_receipt_digest,
    ] {
        if Sha256Digest::parse(digest.as_str())? != *digest {
            return Err("object namespace deletion plan contains a non-canonical digest".into());
        }
    }
    Ok(())
}

/// Terminal cleanup evidence for one exact namespace. The retained restore
/// observation is separate so source cleanup cannot masquerade as backup loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceDeletionEvidence {
    pub deletion_plan_digest: Sha256Digest,
    pub deleted_namespace_id: StorageNamespaceId,
    pub retained_restore_namespace_id: StorageNamespaceId,
    pub state_cleanup_receipt_digest: Sha256Digest,
    pub recovery_cleanup_receipt_digest: Sha256Digest,
    pub namespace_absence_receipt_digest: Sha256Digest,
    pub retained_restore_observation_digest: Sha256Digest,
    pub completed_at: DateTime<Utc>,
    digest: Sha256Digest,
}

impl ObjectNamespaceDeletionEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn complete(
        plan: &ObjectNamespaceDeletionPlan,
        state_cleanup_receipt_digest: Sha256Digest,
        recovery_cleanup_receipt_digest: Sha256Digest,
        namespace_absence_receipt_digest: Sha256Digest,
        retained_restore_observation_digest: Sha256Digest,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        plan.validate()?;
        let mut evidence = Self {
            deletion_plan_digest: plan.digest().clone(),
            deleted_namespace_id: plan.spec.namespace_id,
            retained_restore_namespace_id: plan.spec.retained_restore_namespace_id,
            state_cleanup_receipt_digest,
            recovery_cleanup_receipt_digest,
            namespace_absence_receipt_digest,
            retained_restore_observation_digest,
            completed_at: canonical_timestamp(completed_at),
            digest: Sha256Digest::from_bytes(b"uninitialized"),
        };
        validate_deletion_evidence_shape(&evidence)?;
        evidence.digest = deletion_evidence_digest(&evidence)?;
        evidence.validate_for(plan)?;
        Ok(evidence)
    }

    pub fn validate_for(&self, plan: &ObjectNamespaceDeletionPlan) -> Result<(), String> {
        plan.validate()?;
        validate_deletion_evidence_shape(self)?;
        if self.digest != deletion_evidence_digest(self)?
            || self.deletion_plan_digest != *plan.digest()
            || self.deleted_namespace_id != plan.spec.namespace_id
            || self.retained_restore_namespace_id != plan.spec.retained_restore_namespace_id
            || self.completed_at < plan.spec.not_before
        {
            return Err("object namespace deletion evidence does not prove the exact plan".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_deletion_evidence_shape(
    evidence: &ObjectNamespaceDeletionEvidence,
) -> Result<(), String> {
    if evidence.deleted_namespace_id.as_uuid().is_nil()
        || evidence.retained_restore_namespace_id.as_uuid().is_nil()
        || evidence.deleted_namespace_id == evidence.retained_restore_namespace_id
        || evidence.completed_at != canonical_timestamp(evidence.completed_at)
    {
        return Err("object namespace deletion evidence is invalid".into());
    }
    for digest in [
        &evidence.deletion_plan_digest,
        &evidence.state_cleanup_receipt_digest,
        &evidence.recovery_cleanup_receipt_digest,
        &evidence.namespace_absence_receipt_digest,
        &evidence.retained_restore_observation_digest,
    ] {
        if Sha256Digest::parse(digest.as_str())? != *digest {
            return Err(
                "object namespace deletion evidence contains a non-canonical digest".into(),
            );
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct RestoreEvidenceDigestProjection<'a> {
    plan_digest: &'a Sha256Digest,
    source_recovery_point_digest: &'a Sha256Digest,
    source_post_restore_manifest_digest: &'a Sha256Digest,
    target_namespace_id: StorageNamespaceId,
    restored_state_digest: &'a Sha256Digest,
    restored_state_size_bytes: u64,
    provider_receipt_digest: &'a Sha256Digest,
    verified_at: DateTime<Utc>,
}

fn restore_evidence_digest(
    evidence: &ObjectNamespaceRestoreEvidence,
) -> Result<Sha256Digest, String> {
    contract_digest(
        &RestoreEvidenceDigestProjection {
            plan_digest: &evidence.plan_digest,
            source_recovery_point_digest: &evidence.source_recovery_point_digest,
            source_post_restore_manifest_digest: &evidence.source_post_restore_manifest_digest,
            target_namespace_id: evidence.target_namespace_id,
            restored_state_digest: &evidence.restored_state_digest,
            restored_state_size_bytes: evidence.restored_state_size_bytes,
            provider_receipt_digest: &evidence.provider_receipt_digest,
            verified_at: evidence.verified_at,
        },
        "object namespace restore evidence",
    )
}

#[derive(Serialize)]
struct DeletionEvidenceDigestProjection<'a> {
    deletion_plan_digest: &'a Sha256Digest,
    deleted_namespace_id: StorageNamespaceId,
    retained_restore_namespace_id: StorageNamespaceId,
    state_cleanup_receipt_digest: &'a Sha256Digest,
    recovery_cleanup_receipt_digest: &'a Sha256Digest,
    namespace_absence_receipt_digest: &'a Sha256Digest,
    retained_restore_observation_digest: &'a Sha256Digest,
    completed_at: DateTime<Utc>,
}

fn deletion_evidence_digest(
    evidence: &ObjectNamespaceDeletionEvidence,
) -> Result<Sha256Digest, String> {
    contract_digest(
        &DeletionEvidenceDigestProjection {
            deletion_plan_digest: &evidence.deletion_plan_digest,
            deleted_namespace_id: evidence.deleted_namespace_id,
            retained_restore_namespace_id: evidence.retained_restore_namespace_id,
            state_cleanup_receipt_digest: &evidence.state_cleanup_receipt_digest,
            recovery_cleanup_receipt_digest: &evidence.recovery_cleanup_receipt_digest,
            namespace_absence_receipt_digest: &evidence.namespace_absence_receipt_digest,
            retained_restore_observation_digest: &evidence.retained_restore_observation_digest,
            completed_at: evidence.completed_at,
        },
        "object namespace deletion evidence",
    )
}

fn contract_digest<T: Serialize>(value: &T, label: &str) -> Result<Sha256Digest, String> {
    let bytes = canonical_json_bounded(value, MAX_RECOVERY_CONTRACT_BYTES, label)?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn policy() -> ObjectNamespaceRetentionPolicy {
        ObjectNamespaceRetentionPolicy::from_spec(
            super::super::ObjectNamespaceRetentionPolicySpec {
                minimum_sealed_recovery_points: 1,
                maximum_sealed_recovery_points: 24,
                maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
                deletion_grace_period_seconds: 60 * 60,
            },
        )
        .expect("policy")
    }

    fn first_point(
        namespace_id: StorageNamespaceId,
        sealed_at: DateTime<Utc>,
    ) -> ObjectNamespaceRecoveryPoint {
        ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
            namespace_id,
            sequence: 1,
            writer_epoch: 7,
            provider_profile_digest: digest('a'),
            manifest_key: ObjectNamespaceKey::parse("recovery/epoch-7/manifest")
                .expect("manifest key"),
            manifest_digest: digest('b'),
            state_digest: digest('c'),
            state_size_bytes: 4096,
            predecessor_digest: None,
            sealed_at,
        })
        .expect("recovery point")
    }

    #[test]
    fn sealed_lineage_rejects_skips_foreign_namespaces_and_stale_epochs() {
        let now = Utc::now();
        let first = first_point(StorageNamespaceId::new(), now);
        let successor = ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
            namespace_id: first.spec.namespace_id,
            sequence: 2,
            writer_epoch: 8,
            provider_profile_digest: digest('a'),
            manifest_key: ObjectNamespaceKey::parse("recovery/epoch-8/manifest")
                .expect("manifest key"),
            manifest_digest: digest('d'),
            state_digest: digest('e'),
            state_size_bytes: 8192,
            predecessor_digest: Some(first.digest().clone()),
            sealed_at: now + Duration::seconds(1),
        })
        .expect("successor");
        successor.validate_successor_of(&first).expect("lineage");

        let mut skipped = successor.spec().clone();
        skipped.sequence = 4;
        skipped.predecessor_digest = Some(successor.digest().clone());
        let skipped = ObjectNamespaceRecoveryPoint::seal(skipped).expect("sealed shape");
        assert!(skipped.validate_successor_of(&successor).is_err());

        let mut stale = successor.spec().clone();
        stale.sequence = 3;
        stale.writer_epoch = 6;
        stale.predecessor_digest = Some(successor.digest().clone());
        let stale = ObjectNamespaceRecoveryPoint::seal(stale).expect("sealed shape");
        assert!(stale.validate_successor_of(&successor).is_err());

        let mut foreign = successor.spec().clone();
        foreign.sequence = 3;
        foreign.namespace_id = StorageNamespaceId::new();
        foreign.predecessor_digest = Some(successor.digest().clone());
        let foreign = ObjectNamespaceRecoveryPoint::seal(foreign).expect("sealed shape");
        assert!(foreign.validate_successor_of(&successor).is_err());
    }

    #[test]
    fn restore_and_delete_require_isolation_exact_evidence_and_grace() {
        let now = canonical_timestamp(Utc::now());
        let point = first_point(StorageNamespaceId::new(), now);
        let policy = policy();
        let target_namespace_id = StorageNamespaceId::new();
        let restore_plan = ObjectNamespaceRestorePlan::for_recovery_point(
            &point,
            target_namespace_id,
            digest('f'),
            &policy,
            now + Duration::seconds(1),
        )
        .expect("restore plan");
        let restore_evidence = ObjectNamespaceRestoreEvidence::verified(
            &restore_plan,
            digest('1'),
            now + Duration::seconds(2),
        )
        .expect("restore evidence");
        let delete_plan = ObjectNamespaceDeletionPlan::after_verified_restore(
            &point,
            &restore_plan,
            &restore_evidence,
            &policy,
            digest('2'),
            digest('3'),
            now + Duration::seconds(3),
        )
        .expect("deletion plan");
        assert!(delete_plan.spec.not_before > delete_plan.spec.requested_at);

        assert!(ObjectNamespaceDeletionEvidence::complete(
            &delete_plan,
            digest('4'),
            digest('5'),
            digest('6'),
            digest('7'),
            delete_plan.spec.not_before - Duration::microseconds(1),
        )
        .is_err());
        let evidence = ObjectNamespaceDeletionEvidence::complete(
            &delete_plan,
            digest('4'),
            digest('5'),
            digest('6'),
            digest('7'),
            delete_plan.spec.not_before,
        )
        .expect("deletion evidence");
        evidence.validate_for(&delete_plan).expect("exact deletion");
        assert_eq!(evidence.deleted_namespace_id, point.spec.namespace_id);
        assert_eq!(evidence.retained_restore_namespace_id, target_namespace_id);

        assert!(ObjectNamespaceRestorePlan::for_recovery_point(
            &point,
            point.spec.namespace_id,
            digest('f'),
            &policy,
            now + Duration::seconds(1),
        )
        .is_err());

        let foreign_point = first_point(StorageNamespaceId::new(), now);
        assert!(delete_plan
            .validate_against(&foreign_point, &restore_plan, &restore_evidence, &policy)
            .is_err());
    }

    #[test]
    fn persisted_contracts_reject_digest_or_timestamp_drift() {
        let now = canonical_timestamp(Utc::now());
        let point = first_point(StorageNamespaceId::new(), now);
        assert!(ObjectNamespaceRecoveryPoint::restore(
            point.spec().clone(),
            &format!("sha256:{}", "9".repeat(64))
        )
        .is_err());

        let mut noncanonical = point.spec().clone();
        noncanonical.sealed_at += Duration::nanoseconds(1);
        assert!(
            ObjectNamespaceRecoveryPoint::restore(noncanonical, point.digest().as_str()).is_err()
        );
    }

    #[test]
    fn recovery_contract_does_not_create_an_object_client_or_lifecycle() {
        let source = include_str!("object_namespace_recovery.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "object_store::",
            "ImmutableObjectClient",
            "ISecretRepository",
            "IFlowClient",
            "tokio::spawn",
            "reqwest::",
        ] {
            assert!(
                !production.contains(forbidden),
                "S0 recovery contracts must reuse owning mechanisms; found {forbidden}"
            );
        }
    }
}
