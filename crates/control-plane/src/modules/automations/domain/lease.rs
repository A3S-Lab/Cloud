use super::{
    AutomationScheduleCalculator, AutomationScheduleDueEvaluation, AutomationScheduleDueEvaluator,
};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use a3s_cloud_contracts::AutomationMisfirePolicyV1;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// Maximum time for which one schedule evaluator may hold a due-work lease.
///
/// The durable scheduler owner must enforce the same bound when it persists a
/// claim. Keeping this value here makes stale-worker behavior deterministic for
/// every repository adapter.
pub const AUTOMATION_SCHEDULE_MAX_LEASE_MS: i64 = 5 * 60 * 1_000;

/// One immutable fence for a schedule due-evaluation attempt.
///
/// The fence is a capability fact, not a scheduler row. A repository owner
/// must persist and compare its owner and lease IDs atomically with cursor and
/// invocation decisions.
#[derive(Clone, PartialEq, Eq)]
pub struct AutomationScheduleLease {
    owner_id: Uuid,
    lease_id: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
}

impl AutomationScheduleLease {
    pub fn new(
        owner_id: Uuid,
        lease_id: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let lease = Self {
            owner_id,
            lease_id,
            reserved_at: canonical_timestamp(reserved_at),
            lease_expires_at: canonical_timestamp(lease_expires_at),
        };
        lease.validate()?;
        Ok(lease)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.owner_id.is_nil()
            || self.lease_id.is_nil()
            || self.reserved_at != canonical_timestamp(self.reserved_at)
            || self.lease_expires_at != canonical_timestamp(self.lease_expires_at)
            || self.lease_expires_at <= self.reserved_at
            || self.lease_expires_at - self.reserved_at
                > Duration::milliseconds(AUTOMATION_SCHEDULE_MAX_LEASE_MS)
        {
            return Err("Automation schedule lease fence is invalid".into());
        }
        Ok(())
    }

    /// Require the exact owner and fence while the half-open lease interval is active.
    pub fn authorize(
        &self,
        owner_id: Uuid,
        lease_id: Uuid,
        observed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.validate()?;
        if owner_id != self.owner_id || lease_id != self.lease_id {
            return Err("Automation schedule lease fence is owned by another evaluator".into());
        }
        let observed_at = canonical_timestamp(observed_at);
        if observed_at < self.reserved_at || observed_at >= self.lease_expires_at {
            return Err("Automation schedule lease has expired or is not active".into());
        }
        Ok(())
    }

    pub const fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub const fn lease_id(&self) -> Uuid {
        self.lease_id
    }

    pub const fn reserved_at(&self) -> DateTime<Utc> {
        self.reserved_at
    }

    pub const fn lease_expires_at(&self) -> DateTime<Utc> {
        self.lease_expires_at
    }
}

impl std::fmt::Debug for AutomationScheduleLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomationScheduleLease")
            .field("owner_id", &self.owner_id)
            .field("lease_id", &"redacted")
            .field("reserved_at", &self.reserved_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .finish()
    }
}

/// Composes the due evaluator with a lease fence and rechecks the fence after
/// calculation. The recheck prevents a worker from publishing a stale result
/// after a long calendar or validation pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationScheduleLeaseEvaluator;

/// Inputs for one lease-guarded due evaluation.
pub struct AutomationScheduleLeaseEvaluationRequest<'a> {
    pub owner_id: Uuid,
    pub lease_id: Uuid,
    pub calculator: &'a AutomationScheduleCalculator,
    pub policy: &'a AutomationMisfirePolicyV1,
    pub cursor: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub limit: usize,
}

impl AutomationScheduleLeaseEvaluator {
    pub fn evaluate(
        lease: &AutomationScheduleLease,
        request: AutomationScheduleLeaseEvaluationRequest<'_>,
    ) -> Result<AutomationScheduleDueEvaluation, String> {
        lease.authorize(request.owner_id, request.lease_id, request.observed_at)?;
        let evaluation = AutomationScheduleDueEvaluator::evaluate(
            request.calculator,
            request.policy,
            request.cursor,
            request.observed_at,
            request.limit,
        )?;
        let checked_at = evaluation
            .evaluated_through
            .unwrap_or(request.observed_at)
            .max(request.observed_at);
        lease.authorize(request.owner_id, request.lease_id, checked_at)?;
        Ok(evaluation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{AutomationMisfireModeV1, AutomationScheduleTriggerV1};

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    fn lease() -> AutomationScheduleLease {
        AutomationScheduleLease::new(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            timestamp(1_000),
            timestamp(1_300),
        )
        .expect("lease")
    }

    fn calculator() -> AutomationScheduleCalculator {
        AutomationScheduleCalculator::new(&AutomationScheduleTriggerV1 {
            expression: "0 * * * * * *".into(),
            timezone: "UTC".into(),
        })
        .expect("calculator")
    }

    #[test]
    fn authorizes_only_exact_active_fence() {
        let lease = lease();
        assert!(lease
            .authorize(Uuid::from_u128(1), Uuid::from_u128(2), timestamp(1_299))
            .is_ok());
        assert!(lease
            .authorize(Uuid::from_u128(1), Uuid::from_u128(2), timestamp(1_300))
            .is_err());
        assert!(lease
            .authorize(Uuid::from_u128(3), Uuid::from_u128(2), timestamp(1_100))
            .is_err());
        assert!(lease
            .authorize(Uuid::from_u128(1), Uuid::from_u128(4), timestamp(1_100))
            .is_err());
    }

    #[test]
    fn rejects_invalid_or_overlong_fences() {
        assert!(AutomationScheduleLease::new(
            Uuid::nil(),
            Uuid::from_u128(2),
            timestamp(1_000),
            timestamp(1_100),
        )
        .is_err());
        assert!(AutomationScheduleLease::new(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            timestamp(1_000),
            timestamp(1_000 + (AUTOMATION_SCHEDULE_MAX_LEASE_MS / 1_000) + 1),
        )
        .is_err());
    }

    #[test]
    fn lease_evaluator_rejects_expired_observation_before_producing_due_work() {
        let policy = AutomationMisfirePolicyV1 {
            mode: AutomationMisfireModeV1::Skip,
            grace_ms: 60_000,
        };
        assert!(AutomationScheduleLeaseEvaluator::evaluate(
            &lease(),
            AutomationScheduleLeaseEvaluationRequest {
                owner_id: Uuid::from_u128(1),
                lease_id: Uuid::from_u128(2),
                calculator: &calculator(),
                policy: &policy,
                cursor: timestamp(1_000),
                observed_at: timestamp(1_300),
                limit: 8,
            },
        )
        .is_err());
    }

    #[test]
    fn lease_evaluator_returns_due_window_while_fence_is_active() {
        let policy = AutomationMisfirePolicyV1 {
            mode: AutomationMisfireModeV1::FireLatest,
            grace_ms: 60_000,
        };
        let result = AutomationScheduleLeaseEvaluator::evaluate(
            &lease(),
            AutomationScheduleLeaseEvaluationRequest {
                owner_id: Uuid::from_u128(1),
                lease_id: Uuid::from_u128(2),
                calculator: &calculator(),
                policy: &policy,
                cursor: timestamp(1_000),
                observed_at: timestamp(1_119),
                limit: 8,
            },
        )
        .expect("leased evaluation");
        assert_eq!(result.selection.selected.len(), 1);
        assert_eq!(result.evaluated_through, Some(timestamp(1_080)));
    }
}
