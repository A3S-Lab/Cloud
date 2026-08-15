use crate::modules::shared_kernel::domain::{canonical_json_bounded, Sha256Digest};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const MAX_RETENTION_POLICY_BYTES: usize = 4 * 1024;
const MAX_RECOVERY_POINTS: u32 = 10_000;
const MINIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 60 * 60;
const MAXIMUM_RECOVERY_POINT_AGE_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;
const MINIMUM_DELETION_GRACE_SECONDS: u64 = 5 * 60;
const MAXIMUM_DELETION_GRACE_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Provider-neutral S0 recovery retention semantics.
///
/// This is a typed internal value, not a product configuration parser. A public
/// configuration surface must still admit canonical A3S ACL and project it into
/// this bounded spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRetentionPolicySpec {
    pub minimum_sealed_recovery_points: u32,
    pub maximum_sealed_recovery_points: u32,
    pub maximum_recovery_point_age_seconds: u64,
    pub deletion_grace_period_seconds: u64,
}

impl ObjectNamespaceRetentionPolicySpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.minimum_sealed_recovery_points == 0
            || self.maximum_sealed_recovery_points < self.minimum_sealed_recovery_points
            || self.maximum_sealed_recovery_points > MAX_RECOVERY_POINTS
            || !(MINIMUM_RECOVERY_POINT_AGE_SECONDS..=MAXIMUM_RECOVERY_POINT_AGE_SECONDS)
                .contains(&self.maximum_recovery_point_age_seconds)
            || !(MINIMUM_DELETION_GRACE_SECONDS..=MAXIMUM_DELETION_GRACE_SECONDS)
                .contains(&self.deletion_grace_period_seconds)
        {
            return Err("object namespace retention policy is outside supported bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceRetentionPolicy {
    spec: ObjectNamespaceRetentionPolicySpec,
    digest: Sha256Digest,
}

impl ObjectNamespaceRetentionPolicy {
    pub fn from_spec(spec: ObjectNamespaceRetentionPolicySpec) -> Result<Self, String> {
        spec.validate()?;
        let digest = policy_digest(&spec)?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: ObjectNamespaceRetentionPolicySpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        let policy = Self::from_spec(spec)?;
        if policy.digest.as_str() != stored_digest {
            return Err("stored object namespace retention policy digest drifted".into());
        }
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if &Self::restore(self.spec.clone(), self.digest.as_str())? != self {
            return Err("object namespace retention policy drifted".into());
        }
        Ok(())
    }

    pub fn deletion_not_before(
        &self,
        requested_at: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, String> {
        self.validate()?;
        requested_at
            .checked_add_signed(Duration::seconds(
                self.spec.deletion_grace_period_seconds as i64,
            ))
            .ok_or_else(|| "object namespace deletion grace period overflowed".into())
    }

    pub fn spec(&self) -> &ObjectNamespaceRetentionPolicySpec {
        &self.spec
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn policy_digest(spec: &ObjectNamespaceRetentionPolicySpec) -> Result<Sha256Digest, String> {
    let bytes = canonical_json_bounded(
        spec,
        MAX_RETENTION_POLICY_BYTES,
        "object namespace retention policy",
    )?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ObjectNamespaceRetentionPolicySpec {
        ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        }
    }

    #[test]
    fn policy_is_bounded_digest_locked_and_has_a_positive_delete_grace() {
        let policy = ObjectNamespaceRetentionPolicy::from_spec(spec()).expect("policy");
        policy.validate().expect("valid policy");
        assert!(ObjectNamespaceRetentionPolicy::restore(
            spec(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());

        let requested_at = Utc::now();
        assert!(
            policy
                .deletion_not_before(requested_at)
                .expect("delete grace")
                > requested_at
        );

        let mut invalid = spec();
        invalid.minimum_sealed_recovery_points = 0;
        assert!(ObjectNamespaceRetentionPolicy::from_spec(invalid).is_err());
        let mut invalid = spec();
        invalid.maximum_sealed_recovery_points = MAX_RECOVERY_POINTS + 1;
        assert!(ObjectNamespaceRetentionPolicy::from_spec(invalid).is_err());
    }
}
