use chrono::{DateTime, Timelike, Utc};

pub(crate) fn canonical_timestamp(
    label: &str,
    value: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    value
        .with_nanosecond(value.nanosecond() / 1_000 * 1_000)
        .ok_or_else(|| format!("{label} timestamp is outside supported bounds"))
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

        assert_eq!(
            canonical_timestamp("fixture", timestamp)
                .expect("canonical timestamp")
                .nanosecond(),
            123_456_000
        );
    }
}
