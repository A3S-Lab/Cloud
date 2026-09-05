use a3s_cloud_contracts::{AutomationScheduleTriggerV1, AutomationTriggerV1};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use std::str::FromStr;

/// Maximum number of calendar occurrences returned by one component query.
///
/// The calculator is intentionally bounded and stateless.  It does not lease,
/// enqueue, persist, or publish an invocation; those responsibilities remain
/// with the Automations scheduler/application boundary.
pub const AUTOMATION_SCHEDULE_MAX_OCCURRENCES: usize = 1_024;

/// Stateless calendar calculator for one immutable schedule trigger.
///
/// This adapter translates the canonical seven-field Automation expression
/// into UTC instants using the trigger's canonical IANA timezone.  It is a
/// deterministic building block for a future due-evaluation owner and does
/// not implement misfire, concurrency, leases, or scheduler persistence.
#[derive(Debug, Clone)]
pub struct AutomationScheduleCalculator {
    schedule: Schedule,
    timezone: Tz,
}

impl AutomationScheduleCalculator {
    /// Parse and validate one schedule trigger.
    pub fn new(trigger: &AutomationScheduleTriggerV1) -> Result<Self, String> {
        AutomationTriggerV1::Schedule(trigger.clone())
            .validate()
            .map_err(|error| format!("Automation schedule trigger is invalid: {error}"))?;
        let schedule = Schedule::from_str(&trigger.expression).map_err(|_| {
            "Automation schedule expression cannot be parsed by the fixed calendar evaluator"
                .to_owned()
        })?;
        let timezone = Tz::from_str(&trigger.timezone).map_err(|_| {
            "Automation schedule timezone is not a supported IANA timezone".to_owned()
        })?;
        if timezone.name() != trigger.timezone {
            return Err("Automation schedule timezone name is not canonical".into());
        }
        Ok(Self { schedule, timezone })
    }

    /// Return the first occurrence strictly after `after` in UTC.
    pub fn next_after(&self, after: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        self.schedule
            .after(&after.with_timezone(&self.timezone))
            .next()
            .map(|occurrence| occurrence.with_timezone(&Utc))
            .ok_or_else(|| {
                "Automation schedule has no occurrence after the supplied instant".into()
            })
    }

    /// Return at most `limit` occurrences in `(start, end]` in UTC order.
    ///
    /// The exclusive lower bound makes replay cursors unambiguous, while the
    /// inclusive upper bound lets a due evaluator ask for occurrences through
    /// its observation instant without adding a second ad-hoc tick.
    pub fn occurrences_between(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DateTime<Utc>>, String> {
        if end <= start {
            return Err("Automation schedule occurrence window must move forward".into());
        }
        if limit == 0 || limit > AUTOMATION_SCHEDULE_MAX_OCCURRENCES {
            return Err("Automation schedule occurrence limit is outside its bound".into());
        }

        let mut occurrences = Vec::with_capacity(limit.min(16));
        for occurrence in self
            .schedule
            .after(&start.with_timezone(&self.timezone))
            .take(limit)
        {
            let occurrence = occurrence.with_timezone(&Utc);
            if occurrence > end {
                break;
            }
            occurrences.push(occurrence);
        }

        Ok(occurrences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn trigger(expression: &str, timezone: &str) -> AutomationScheduleTriggerV1 {
        AutomationScheduleTriggerV1 {
            expression: expression.into(),
            timezone: timezone.into(),
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn calculates_the_next_occurrence_in_utc_from_the_trigger_timezone() {
        let calculator =
            AutomationScheduleCalculator::new(&trigger("0 0 9 * * * *", "Asia/Shanghai"))
                .expect("calculator");
        assert_eq!(
            calculator
                .next_after(instant("2026-01-01T00:00:00.000Z"))
                .expect("next occurrence"),
            instant("2026-01-01T01:00:00.000Z")
        );
    }

    #[test]
    fn preserves_both_instants_for_a_fall_back_ambiguous_hour() {
        let calculator =
            AutomationScheduleCalculator::new(&trigger("0 30 1 * * * *", "America/New_York"))
                .expect("calculator");
        let occurrences = calculator
            .occurrences_between(
                instant("2026-11-01T04:00:00.000Z"),
                instant("2026-11-01T07:00:00.000Z"),
                8,
            )
            .expect("occurrences");
        assert_eq!(
            occurrences,
            vec![
                instant("2026-11-01T05:30:00.000Z"),
                instant("2026-11-01T06:30:00.000Z")
            ]
        );
    }

    #[test]
    fn rejects_invalid_timezones_windows_and_unbounded_limits() {
        assert!(
            AutomationScheduleCalculator::new(&trigger("0 0 9 * * * *", "Not/AZone",)).is_err()
        );
        let calculator = AutomationScheduleCalculator::new(&trigger("0 0 9 * * * *", "UTC"))
            .expect("calculator");
        let start = Utc
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp");
        assert!(calculator.occurrences_between(start, start, 1).is_err());
        assert!(calculator
            .occurrences_between(start, start + chrono::Duration::hours(1), 0)
            .is_err());
        assert!(calculator
            .occurrences_between(
                start,
                start + chrono::Duration::hours(1),
                AUTOMATION_SCHEDULE_MAX_OCCURRENCES + 1,
            )
            .is_err());
    }
}
