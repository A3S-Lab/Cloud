use chrono::{DateTime, TimeDelta, Timelike, Utc};

/// Canonicalize a timestamp to PostgreSQL's microsecond precision.
///
/// The subtraction never crosses the current second, so every valid UTC
/// timestamp has a canonical representation without a fallible conversion.
pub(crate) fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value - TimeDelta::nanoseconds(i64::from(value.nanosecond() % 1_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn canonicalizes_to_microseconds() {
        let timestamp = Utc
            .timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .expect("timestamp");

        assert_eq!(canonical_timestamp(timestamp).nanosecond(), 123_456_000);
    }
}
