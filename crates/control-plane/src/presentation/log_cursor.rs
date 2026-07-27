const LOG_CURSOR_PREFIX: &str = "v1:";

pub(crate) fn parse_log_cursor(cursor: &str) -> Option<u64> {
    cursor
        .strip_prefix(LOG_CURSOR_PREFIX)
        .filter(|sequence| !sequence.is_empty())
        .and_then(|sequence| sequence.parse::<u64>().ok())
}

pub(crate) fn format_log_cursor(sequence: u64) -> String {
    format!("{LOG_CURSOR_PREFIX}{sequence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_cursor_round_trip_is_canonical_and_bounded_to_u64() {
        for sequence in [0, 1, u64::MAX] {
            let cursor = format_log_cursor(sequence);
            assert_eq!(parse_log_cursor(&cursor), Some(sequence));
        }
        for invalid in ["", "v1:", "1", "v2:1", "v1:-1", "v1:18446744073709551616"] {
            assert_eq!(parse_log_cursor(invalid), None);
        }
    }
}
