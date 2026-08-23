use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, Sha256Digest};
use chrono::{DateTime, Utc};
use std::time::Duration;

use super::{AuditRecordCursor, AuditRecordFilter};

pub const AUDIT_RETENTION_POLICY_SCHEMA: &str = "a3s.cloud.audit-retention-policy.v1";
pub const MINIMUM_AUDIT_RETENTION_MS: u64 = 86_400_000;
pub const MAXIMUM_AUDIT_RETENTION_MS: u64 = 315_576_000_000;
pub const MAXIMUM_AUDIT_RETENTION_BATCH_SIZE: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRetentionPolicy {
    retention_ms: u64,
    digest: Sha256Digest,
}

impl AuditRetentionPolicy {
    pub fn new(retention: Duration) -> Result<Self, String> {
        let retention_ms = u64::try_from(retention.as_millis())
            .map_err(|_| "audit retention duration exceeds supported bounds")?;
        if Duration::from_millis(retention_ms) != retention
            || !(MINIMUM_AUDIT_RETENTION_MS..=MAXIMUM_AUDIT_RETENTION_MS).contains(&retention_ms)
        {
            return Err("audit retention must be an exact duration from 1 day to 10 years".into());
        }
        let canonical =
            format!("schema={AUDIT_RETENTION_POLICY_SCHEMA}\nretention_ms={retention_ms}\n");
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
            .map_err(|_| "audit retention duration exceeds supported bounds")?;
        canonical_timestamp(now)
            .checked_sub_signed(retention)
            .map(canonical_timestamp)
            .ok_or_else(|| "audit retention cutoff overflowed".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRetentionState {
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

impl AuditRetentionState {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() {
            return Err("audit retention organization ID must not be nil".into());
        }
        if self
            .records_deleted_before
            .zip(self.records_available_from)
            .is_some_and(|(deleted, available)| deleted > available)
        {
            return Err(
                "audit retention deletion boundary exceeds its availability boundary".into(),
            );
        }
        if self.records_deleted_before.is_some() && self.records_available_from.is_none() {
            return Err(
                "audit retention deletion boundary requires an availability boundary".into(),
            );
        }
        if self.last_completed_at.is_some() && self.records_deleted_before.is_none() {
            return Err("audit retention completion requires a deletion boundary".into());
        }
        let initialized = self.records_available_from.is_some();
        if initialized != self.applied_policy_digest.is_some()
            || initialized != self.last_swept_at.is_some()
        {
            return Err(
                "audit retention availability, applied policy, and sweep time must initialize together"
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
            return Err("audit retention sweep schedule is inconsistent".into());
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
            return Err("audit retention state timestamps must use canonical precision".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRetentionStatus {
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

impl AuditRetentionStatus {
    pub fn from_state(
        policy: &AuditRetentionPolicy,
        state: AuditRetentionState,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRetentionSweep {
    pub cutoff: DateTime<Utc>,
    pub swept_at: DateTime<Utc>,
    pub next_scan_at: DateTime<Utc>,
    pub policy_digest: Sha256Digest,
    pub organization_batch_size: usize,
    pub record_batch_size: usize,
}

impl AuditRetentionSweep {
    pub fn validate(&self) -> Result<(), String> {
        if self.cutoff > self.swept_at || self.next_scan_at <= self.swept_at {
            return Err("audit retention sweep timestamps are invalid".into());
        }
        if self.organization_batch_size == 0
            || self.organization_batch_size > MAXIMUM_AUDIT_RETENTION_BATCH_SIZE
            || self.record_batch_size == 0
            || self.record_batch_size > MAXIMUM_AUDIT_RETENTION_BATCH_SIZE
        {
            return Err("audit retention sweep batches must be between 1 and 10000".into());
        }
        if canonical_timestamp(self.cutoff) != self.cutoff
            || canonical_timestamp(self.swept_at) != self.swept_at
            || canonical_timestamp(self.next_scan_at) != self.next_scan_at
        {
            return Err("audit retention sweep timestamps must use canonical precision".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRetentionReport {
    pub inspected_organizations: usize,
    pub completed_organizations: usize,
    pub deleted_records: usize,
}

pub(crate) fn validate_retained_query_window(
    records_available_from: Option<DateTime<Utc>>,
    filter: &AuditRecordFilter,
    after: Option<AuditRecordCursor>,
) -> Result<(), String> {
    let Some(boundary) = records_available_from else {
        return Ok(());
    };
    if filter.from.is_some_and(|from| from < boundary)
        || filter.to.is_some_and(|to| to < boundary)
        || after.is_some_and(|cursor| cursor.occurred_at < boundary)
    {
        return Err(format!(
            "audit records before {} are no longer available",
            boundary.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn policy_digest_is_semantic_and_cutoff_is_canonical() {
        let policy =
            AuditRetentionPolicy::new(Duration::from_secs(90 * 24 * 60 * 60)).expect("policy");
        assert_eq!(policy.retention_ms(), 7_776_000_000);
        assert_eq!(
            policy.digest(),
            &Sha256Digest::from_bytes(
                b"schema=a3s.cloud.audit-retention-policy.v1\nretention_ms=7776000000\n"
            )
        );
        let now = Utc
            .timestamp_opt(1_800_000_000, 123_456_789)
            .single()
            .expect("now");
        assert_eq!(
            policy.cutoff(now).expect("cutoff").timestamp_subsec_nanos(),
            123_456_000
        );
    }

    #[test]
    fn rejects_unbounded_or_sub_millisecond_policy_and_sweep_values() {
        for retention in [
            Duration::from_secs(1),
            Duration::from_secs(24 * 60 * 60) + Duration::from_nanos(1),
            Duration::from_millis(MAXIMUM_AUDIT_RETENTION_MS + 1),
        ] {
            assert!(AuditRetentionPolicy::new(retention).is_err());
        }
        let policy = AuditRetentionPolicy::new(Duration::from_secs(24 * 60 * 60)).expect("policy");
        let now = canonical_timestamp(Utc::now());
        assert!(AuditRetentionSweep {
            cutoff: now,
            swept_at: now,
            next_scan_at: now,
            policy_digest: policy.digest().clone(),
            organization_batch_size: 1,
            record_batch_size: 1,
        }
        .validate()
        .is_err());
    }
}
