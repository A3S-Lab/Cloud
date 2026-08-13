pub(crate) fn validate_audit_action(value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 255
        && value.split('.').count() >= 3
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err("audit action must use bounded lowercase dot-separated segments".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_bounded_actions() {
        assert_eq!(validate_audit_action("identity.membership.created"), Ok(()));
        for invalid in [
            "identity.created",
            "Identity.membership.created",
            "identity.membership created",
            "identity..created",
            "",
        ] {
            assert!(validate_audit_action(invalid).is_err(), "{invalid}");
        }
        assert!(validate_audit_action(&"a".repeat(256)).is_err());
    }
}
