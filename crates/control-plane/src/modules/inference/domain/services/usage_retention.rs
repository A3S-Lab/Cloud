//! Inference usage retention — hide showback, then purge durable rows.
//!
//! Watermarks are never deleted. Event digests may be forgotten only after the
//! availability boundary advances; Gateway must not re-send ACKed cursors.

use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, Sha256Digest};
use chrono::{DateTime, NaiveDate, Utc};
use std::time::Duration;

pub const INFERENCE_USAGE_RETENTION_POLICY_SCHEMA: &str =
    "a3s.cloud.inference-usage-retention-policy.v1";
pub const MINIMUM_INFERENCE_USAGE_RETENTION_MS: u64 = 86_400_000;
pub const MAXIMUM_INFERENCE_USAGE_RETENTION_MS: u64 = 315_576_000_000;
pub const MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageRetentionPolicy {
    retention_ms: u64,
    digest: Sha256Digest,
}

impl InferenceUsageRetentionPolicy {
    pub fn new(retention: Duration) -> Result<Self, String> {
        let retention_ms = u64::try_from(retention.as_millis())
            .map_err(|_| "inference usage retention duration exceeds supported bounds")?;
        if Duration::from_millis(retention_ms) != retention
            || !(MINIMUM_INFERENCE_USAGE_RETENTION_MS..=MAXIMUM_INFERENCE_USAGE_RETENTION_MS)
                .contains(&retention_ms)
        {
            return Err(
                "inference usage retention must be an exact duration from 1 day to 10 years".into(),
            );
        }
        let canonical = format!(
            "schema={INFERENCE_USAGE_RETENTION_POLICY_SCHEMA}\nretention_ms={retention_ms}\n"
        );
        Ok(Self {
            retention_ms,
            digest: Sha256Digest::from_bytes(canonical.as_bytes()),
        })
    }

    pub const fn retention_ms(&self) -> u64 {
        self.retention_ms
    }

    pub const fn retention(&self) -> Duration {
        Duration::from_millis(self.retention_ms)
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn cutoff(&self, now: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let retention = chrono::Duration::from_std(self.retention())
            .map_err(|_| "inference usage retention duration exceeds supported bounds")?;
        canonical_timestamp(now)
            .checked_sub_signed(retention)
            .map(canonical_timestamp)
            .ok_or_else(|| "inference usage retention cutoff overflowed".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageRetentionState {
    pub organization_id: OrganizationId,
    pub records_available_from: Option<DateTime<Utc>>,
    pub records_deleted_before: Option<DateTime<Utc>>,
    pub applied_policy_digest: Option<Sha256Digest>,
    pub total_deleted_records: u64,
    pub last_swept_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub next_scan_at: DateTime<Utc>,
    pub version: u64,
}

impl InferenceUsageRetentionState {
    pub fn initial(organization_id: OrganizationId) -> Self {
        Self {
            organization_id,
            records_available_from: None,
            records_deleted_before: None,
            applied_policy_digest: None,
            total_deleted_records: 0,
            last_swept_at: None,
            last_completed_at: None,
            next_scan_at: DateTime::<Utc>::from_timestamp(0, 0).expect("Unix epoch"),
            version: 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() {
            return Err("inference usage retention organization ID must not be nil".into());
        }
        if self
            .records_deleted_before
            .zip(self.records_available_from)
            .is_some_and(|(deleted, available)| deleted > available)
        {
            return Err(
                "inference usage retention deletion boundary exceeds its availability boundary"
                    .into(),
            );
        }
        if self.records_deleted_before.is_some() && self.records_available_from.is_none() {
            return Err(
                "inference usage retention deletion boundary requires an availability boundary"
                    .into(),
            );
        }
        if self.last_completed_at.is_some() && self.records_deleted_before.is_none() {
            return Err("inference usage retention completion requires a deletion boundary".into());
        }
        let initialized = self.records_available_from.is_some();
        if initialized != self.applied_policy_digest.is_some()
            || initialized != self.last_swept_at.is_some()
        {
            return Err(
                "inference usage retention availability, applied policy, and sweep time must initialize together"
                    .into(),
            );
        }
        if self
            .last_swept_at
            .is_some_and(|last_swept_at| self.next_scan_at <= last_swept_at)
            || self
                .last_completed_at
                .zip(self.last_swept_at)
                .is_some_and(|(completed, swept)| completed > swept)
            || (self.last_completed_at.is_some() && self.last_swept_at.is_none())
        {
            return Err("inference usage retention sweep schedule is inconsistent".into());
        }
        if canonical_timestamp(self.next_scan_at) != self.next_scan_at
            || [
                self.records_available_from,
                self.records_deleted_before,
                self.last_swept_at,
                self.last_completed_at,
            ]
            .into_iter()
            .flatten()
            .any(|timestamp| canonical_timestamp(timestamp) != timestamp)
        {
            return Err(
                "inference usage retention state timestamps must use canonical precision".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageRetentionSweep {
    pub cutoff: DateTime<Utc>,
    pub swept_at: DateTime<Utc>,
    pub next_scan_at: DateTime<Utc>,
    pub policy_digest: Sha256Digest,
    pub organization_batch_size: usize,
    pub record_batch_size: usize,
}

impl InferenceUsageRetentionSweep {
    pub fn validate(&self) -> Result<(), String> {
        if self.cutoff > self.swept_at || self.next_scan_at <= self.swept_at {
            return Err("inference usage retention sweep timestamps are invalid".into());
        }
        if self.organization_batch_size == 0
            || self.organization_batch_size > MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE
            || self.record_batch_size == 0
            || self.record_batch_size > MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE
        {
            return Err(
                "inference usage retention sweep batches must be between 1 and 10000".into(),
            );
        }
        if canonical_timestamp(self.cutoff) != self.cutoff
            || canonical_timestamp(self.swept_at) != self.swept_at
            || canonical_timestamp(self.next_scan_at) != self.next_scan_at
        {
            return Err(
                "inference usage retention sweep timestamps must use canonical precision".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceUsageRetentionReport {
    pub inspected_organizations: usize,
    pub completed_organizations: usize,
    pub deleted_records: usize,
}

/// Showback windows must not read before the hide boundary.
pub fn validate_showback_day_window(
    records_available_from: Option<DateTime<Utc>>,
    from_day: NaiveDate,
    to_day: NaiveDate,
) -> Result<(), String> {
    if from_day > to_day {
        return Err("inference usage rollup from_day must be <= to_day".into());
    }
    let Some(boundary) = records_available_from else {
        return Ok(());
    };
    let available_day = boundary.date_naive();
    if from_day < available_day || to_day < available_day {
        return Err(format!(
            "inference usage showback window precedes retention availability boundary {available_day}"
        ));
    }
    Ok(())
}

pub fn validate_showback_fact_timestamp(
    records_available_from: Option<DateTime<Utc>>,
    started_at: DateTime<Utc>,
) -> Result<(), String> {
    let Some(boundary) = records_available_from else {
        return Ok(());
    };
    if started_at < boundary {
        return Err(format!(
            "inference usage request fact precedes retention availability boundary {boundary}"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageRetentionStatus {
    pub organization_id: OrganizationId,
    pub retention_ms: u64,
    pub policy_digest: Sha256Digest,
    pub applied_policy_digest: Option<Sha256Digest>,
    pub current_policy_applied: bool,
    pub records_available_from: Option<DateTime<Utc>>,
    pub records_deleted_before: Option<DateTime<Utc>>,
    pub total_deleted_records: u64,
    pub last_swept_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub next_scan_at: DateTime<Utc>,
    pub version: u64,
}

impl InferenceUsageRetentionStatus {
    pub fn from_state(
        policy: &InferenceUsageRetentionPolicy,
        state: InferenceUsageRetentionState,
    ) -> Result<Self, String> {
        state.validate()?;
        let current_policy_applied = state.applied_policy_digest.as_ref() == Some(policy.digest());
        Ok(Self {
            organization_id: state.organization_id,
            retention_ms: policy.retention_ms(),
            policy_digest: policy.digest().clone(),
            applied_policy_digest: state.applied_policy_digest,
            current_policy_applied,
            records_available_from: state.records_available_from,
            records_deleted_before: state.records_deleted_before,
            total_deleted_records: state.total_deleted_records,
            last_swept_at: state.last_swept_at,
            last_completed_at: state.last_completed_at,
            next_scan_at: state.next_scan_at,
            version: state.version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn policy_digest_is_semantic_and_cutoff_is_canonical() {
        let policy =
            InferenceUsageRetentionPolicy::new(Duration::from_millis(86_400_000)).expect("policy");
        let again =
            InferenceUsageRetentionPolicy::new(Duration::from_millis(86_400_000)).expect("policy");
        assert_eq!(policy.digest(), again.digest());
        let now = Utc.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap();
        let cutoff = policy.cutoff(now).expect("cutoff");
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 3, 9, 12, 0, 0).unwrap());
        assert_eq!(cutoff, canonical_timestamp(cutoff));
    }

    #[test]
    fn rejects_unbounded_or_sub_day_retention_policy() {
        assert!(InferenceUsageRetentionPolicy::new(Duration::from_millis(86_399_999)).is_err());
        assert!(
            InferenceUsageRetentionPolicy::new(Duration::from_millis(315_576_000_001)).is_err()
        );
    }

    #[test]
    fn showback_window_conflicts_when_from_day_precedes_availability_boundary() {
        let boundary = Utc.with_ymd_and_hms(2026, 1, 10, 0, 0, 0).unwrap();
        let err = validate_showback_day_window(
            Some(boundary),
            NaiveDate::from_ymd_opt(2026, 1, 9).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 11).unwrap(),
        )
        .expect_err("stale window");
        assert!(err.contains("availability boundary"));
        validate_showback_day_window(
            Some(boundary),
            NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 11).unwrap(),
        )
        .expect("on boundary");
    }

    #[test]
    fn retention_state_rejects_backward_deletion_boundary() {
        let mut state =
            InferenceUsageRetentionState::initial(OrganizationId::from_uuid(Uuid::from_u128(1)));
        state.records_available_from = Some(Utc.with_ymd_and_hms(2026, 1, 10, 0, 0, 0).unwrap());
        state.records_deleted_before = Some(Utc.with_ymd_and_hms(2026, 1, 11, 0, 0, 0).unwrap());
        state.applied_policy_digest = Some(Sha256Digest::from_bytes(b"policy"));
        state.last_swept_at = Some(Utc.with_ymd_and_hms(2026, 1, 12, 0, 0, 0).unwrap());
        state.next_scan_at = Utc.with_ymd_and_hms(2026, 1, 12, 1, 0, 0).unwrap();
        assert!(state.validate().is_err());
    }
}
