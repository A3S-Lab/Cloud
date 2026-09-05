use super::{
    AutomationScheduleCalculator, AutomationScheduleDueSelection,
    AutomationScheduleMisfireEvaluator,
};
use a3s_cloud_contracts::AutomationMisfirePolicyV1;
use chrono::{DateTime, Utc};

/// The result of one bounded, cursor-exclusive due-window evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationScheduleDueEvaluation {
    /// The occurrences selected or skipped by the immutable misfire policy.
    pub selection: AutomationScheduleDueSelection,
    /// The last calendar occurrence actually examined by this call.
    ///
    /// A durable owner may advance its cursor to this value in the same
    /// transaction as its invocation decision. `None` means the window had no
    /// occurrence and the cursor must remain unchanged.
    pub evaluated_through: Option<DateTime<Utc>>,
}

/// Stateless composition of calendar calculation and misfire policy.
///
/// The lower cursor bound is exclusive, so replaying a committed cursor never
/// returns the same occurrence twice. The evaluator does not persist the
/// cursor, acquire a lease, count concurrency, enqueue work, or admit an
/// invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationScheduleDueEvaluator;

impl AutomationScheduleDueEvaluator {
    /// Evaluate at most `limit` occurrences through `observed_at`.
    pub fn evaluate(
        calculator: &AutomationScheduleCalculator,
        policy: &AutomationMisfirePolicyV1,
        cursor: DateTime<Utc>,
        observed_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<AutomationScheduleDueEvaluation, String> {
        let occurrences = calculator.occurrences_between(cursor, observed_at, limit)?;
        let evaluated_through = occurrences.last().copied();
        let selection =
            AutomationScheduleMisfireEvaluator::select(policy, &occurrences, observed_at)?;
        Ok(AutomationScheduleDueEvaluation {
            selection,
            evaluated_through,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{AutomationMisfireModeV1, AutomationScheduleTriggerV1};
    use chrono::DateTime;

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn calculator() -> AutomationScheduleCalculator {
        AutomationScheduleCalculator::new(&AutomationScheduleTriggerV1 {
            expression: "0 * * * * * *".into(),
            timezone: "UTC".into(),
        })
        .expect("calculator")
    }

    #[test]
    fn evaluates_cursor_exclusive_window_and_applies_misfire_policy() {
        let result = AutomationScheduleDueEvaluator::evaluate(
            &calculator(),
            &AutomationMisfirePolicyV1 {
                mode: AutomationMisfireModeV1::FireLatest,
                grace_ms: 60_000,
            },
            instant("2026-01-01T10:00:00Z"),
            instant("2026-01-01T10:03:30Z"),
            8,
        )
        .expect("evaluation");
        assert_eq!(
            result.selection.selected,
            vec![instant("2026-01-01T10:03:00Z")]
        );
        assert_eq!(
            result.selection.skipped,
            vec![
                instant("2026-01-01T10:01:00Z"),
                instant("2026-01-01T10:02:00Z")
            ]
        );
        assert_eq!(
            result.evaluated_through,
            Some(instant("2026-01-01T10:03:00Z"))
        );
    }

    #[test]
    fn bounded_limit_reports_only_the_cursor_progress_it_examined() {
        let result = AutomationScheduleDueEvaluator::evaluate(
            &calculator(),
            &AutomationMisfirePolicyV1 {
                mode: AutomationMisfireModeV1::Skip,
                grace_ms: 300_000,
            },
            instant("2026-01-01T10:00:00Z"),
            instant("2026-01-01T10:05:00Z"),
            2,
        )
        .expect("evaluation");
        assert_eq!(result.selection.selected.len(), 2);
        assert!(result.selection.skipped.is_empty());
        assert_eq!(
            result.evaluated_through,
            Some(instant("2026-01-01T10:02:00Z"))
        );
    }

    #[test]
    fn rejects_non_forward_windows_and_zero_limits() {
        let value = calculator();
        let policy = AutomationMisfirePolicyV1 {
            mode: AutomationMisfireModeV1::Skip,
            grace_ms: 0,
        };
        assert!(AutomationScheduleDueEvaluator::evaluate(
            &value,
            &policy,
            instant("2026-01-01T10:00:00Z"),
            instant("2026-01-01T10:00:00Z"),
            1,
        )
        .is_err());
        assert!(AutomationScheduleDueEvaluator::evaluate(
            &value,
            &policy,
            instant("2026-01-01T10:00:00Z"),
            instant("2026-01-01T10:01:00Z"),
            0,
        )
        .is_err());
    }
}
