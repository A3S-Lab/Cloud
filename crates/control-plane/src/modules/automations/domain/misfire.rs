use super::schedule::AUTOMATION_SCHEDULE_MAX_OCCURRENCES;
use a3s_cloud_contracts::{
    AutomationMisfireModeV1, AutomationMisfirePolicyV1, AUTOMATION_MAX_MISFIRE_GRACE_MS,
};
use chrono::{DateTime, Duration, Utc};

/// The deterministic result of evaluating one bounded schedule observation.
///
/// `selected` contains the occurrences that a later scheduler owner may turn
/// into invocations. `skipped` contains occurrences discarded by the grace
/// window or by the coalescing mode. No state is persisted and no invocation is
/// admitted by this value object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationScheduleDueSelection {
    pub selected: Vec<DateTime<Utc>>,
    pub skipped: Vec<DateTime<Utc>>,
}

/// Stateless misfire policy evaluator for a bounded, already-calculated
/// schedule window.
///
/// The caller supplies the durable cursor/window and observation instant. This
/// component only applies the canonical grace and coalescing rules; it does
/// not own a cursor, lease, concurrency counter, persistence, or worker.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationScheduleMisfireEvaluator;

impl AutomationScheduleMisfireEvaluator {
    /// Select occurrences that are still inside the policy grace window.
    ///
    /// Occurrences must be strictly increasing and no later than
    /// `observed_at`. `skip` retains every occurrence inside the grace window,
    /// `fire_once` retains the earliest one, and `fire_latest` retains the
    /// latest one. All other occurrences are reported in `skipped` so a later
    /// owner can record an explicit decision rather than silently dropping a
    /// backlog.
    pub fn select(
        policy: &AutomationMisfirePolicyV1,
        occurrences: &[DateTime<Utc>],
        observed_at: DateTime<Utc>,
    ) -> Result<AutomationScheduleDueSelection, String> {
        if policy.grace_ms > AUTOMATION_MAX_MISFIRE_GRACE_MS {
            return Err("Automation misfire grace is outside its closed bound".into());
        }
        if occurrences.len() > AUTOMATION_SCHEDULE_MAX_OCCURRENCES {
            return Err("Automation misfire occurrence input exceeds its bound".into());
        }
        if occurrences.windows(2).any(|window| window[0] >= window[1]) {
            return Err("Automation misfire occurrences must be strictly increasing".into());
        }
        if occurrences
            .iter()
            .any(|occurrence| *occurrence > observed_at)
        {
            return Err("Automation misfire occurrence cannot be after observation".into());
        }

        let grace = Duration::milliseconds(
            i64::try_from(policy.grace_ms)
                .map_err(|_| "Automation misfire grace cannot be represented".to_owned())?,
        );
        let mut eligible = Vec::with_capacity(occurrences.len());
        for occurrence in occurrences {
            if observed_at.signed_duration_since(*occurrence) <= grace {
                eligible.push(*occurrence);
            }
        }

        let selected = match policy.mode {
            AutomationMisfireModeV1::Skip => eligible,
            AutomationMisfireModeV1::FireOnce => {
                eligible.first().copied().into_iter().collect::<Vec<_>>()
            }
            AutomationMisfireModeV1::FireLatest => {
                eligible.last().copied().into_iter().collect::<Vec<_>>()
            }
        };
        let selected_set = selected
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let skipped = occurrences
            .iter()
            .copied()
            .filter(|occurrence| !selected_set.contains(occurrence))
            .collect();

        Ok(AutomationScheduleDueSelection { selected, skipped })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn policy(mode: AutomationMisfireModeV1, grace_ms: u64) -> AutomationMisfirePolicyV1 {
        AutomationMisfirePolicyV1 { mode, grace_ms }
    }

    #[test]
    fn skip_keeps_only_occurrences_inside_grace() {
        let occurrences = [
            instant("2026-01-01T10:00:00Z"),
            instant("2026-01-01T10:05:00Z"),
            instant("2026-01-01T10:10:00Z"),
        ];
        let selection = AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::Skip, 120_000),
            &occurrences,
            instant("2026-01-01T10:11:00Z"),
        )
        .expect("selection");
        assert_eq!(selection.selected, vec![occurrences[2]]);
        assert_eq!(selection.skipped, vec![occurrences[0], occurrences[1]]);
    }

    #[test]
    fn fire_once_and_fire_latest_coalesce_eligible_occurrences() {
        let occurrences = [
            instant("2026-01-01T10:09:00Z"),
            instant("2026-01-01T10:10:00Z"),
            instant("2026-01-01T10:11:00Z"),
        ];
        let observed_at = instant("2026-01-01T10:11:30Z");
        let once = AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::FireOnce, 180_000),
            &occurrences,
            observed_at,
        )
        .expect("fire once");
        assert_eq!(once.selected, vec![occurrences[0]]);
        assert_eq!(once.skipped, vec![occurrences[1], occurrences[2]]);

        let latest = AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::FireLatest, 180_000),
            &occurrences,
            observed_at,
        )
        .expect("fire latest");
        assert_eq!(latest.selected, vec![occurrences[2]]);
        assert_eq!(latest.skipped, vec![occurrences[0], occurrences[1]]);
    }

    #[test]
    fn rejects_future_unsorted_and_unbounded_inputs() {
        let first = instant("2026-01-01T10:00:00Z");
        let second = instant("2026-01-01T10:01:00Z");
        let observed_at = instant("2026-01-01T10:02:00Z");
        assert!(AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::Skip, 0),
            &[second, first],
            observed_at,
        )
        .is_err());
        assert!(AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::Skip, 0),
            &[observed_at + Duration::seconds(1)],
            observed_at,
        )
        .is_err());
        assert!(AutomationScheduleMisfireEvaluator::select(
            &policy(
                AutomationMisfireModeV1::Skip,
                AUTOMATION_MAX_MISFIRE_GRACE_MS + 1
            ),
            &[],
            observed_at,
        )
        .is_err());
        let too_many = vec![first; AUTOMATION_SCHEDULE_MAX_OCCURRENCES + 1];
        assert!(AutomationScheduleMisfireEvaluator::select(
            &policy(AutomationMisfireModeV1::Skip, 0),
            &too_many,
            observed_at,
        )
        .is_err());
    }
}
