//! Frozen Edge tokenizer revision and inference ACL header helpers.
//!
//! Gateway fail-closes managed `inference` policy unless
//! `tokenizer_revision` equals this exact string. Cloud's future policy
//! compiler must emit the same attribute; this module is the shared contract
//! identity so Cloud and Gateway cannot silently diverge.

use chrono::{DateTime, SecondsFormat, Utc};

/// Provisional Gateway grant-reservation tokenizer revision.
///
/// This is not a Cloud billing tokenizer. It identifies
/// `a3s.gateway.tokenizer.v1` reservation semantics on Edge.
pub const INFERENCE_TOKENIZER_REVISION_V1: &str = "a3s.gateway.tokenizer.v1";

/// ACL attribute line that every compiled `inference { ... }` policy must
/// include (attribute order is not significant).
pub fn inference_tokenizer_revision_acl_attr() -> String {
    format!("tokenizer_revision = \"{INFERENCE_TOKENIZER_REVISION_V1}\"")
}

/// Fail closed unless `acl_fragment` declares the frozen tokenizer revision.
///
/// Matches a whole trimmed ACL line equal to
/// [`inference_tokenizer_revision_acl_attr`]. Rejects missing or any other
/// revision so a future compiler cannot ship a silent reservation-semantics
/// change under Edge.
pub fn require_inference_tokenizer_revision(acl_fragment: &str) -> Result<(), String> {
    let expected = inference_tokenizer_revision_acl_attr();
    if acl_fragment
        .lines()
        .any(|line| line.trim() == expected.as_str())
    {
        Ok(())
    } else {
        Err(format!(
            "inference ACL must declare {expected}; missing or unknown tokenizer_revision fails closed"
        ))
    }
}

/// Render a grant-empty managed `inference` ACL shell that Gateway will accept.
///
/// This is the first compiler brick: every future Cloud inference policy
/// projection must start from a shell that already carries the frozen
/// tokenizer revision and the snapshot-aligned expiry. Credentials, routes,
/// and workers are omitted until catalog/key compilers land.
pub fn render_inference_policy_shell_acl(expires_at: DateTime<Utc>) -> Result<String, String> {
    if expires_at.timestamp_millis() <= 0 {
        return Err("inference policy expires_at must be a positive UTC timestamp".into());
    }
    let expires = expires_at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let acl = format!(
        "inference {{\n  {}\n  expires_at = \"{expires}\"\n}}\n",
        inference_tokenizer_revision_acl_attr()
    );
    require_inference_tokenizer_revision(&acl)?;
    Ok(acl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn revision_is_stable_and_matches_gateway_contract() {
        assert_eq!(INFERENCE_TOKENIZER_REVISION_V1, "a3s.gateway.tokenizer.v1");
        assert_eq!(
            inference_tokenizer_revision_acl_attr(),
            "tokenizer_revision = \"a3s.gateway.tokenizer.v1\""
        );
    }

    #[test]
    fn require_accepts_exact_acl_attribute() {
        let acl = r#"
inference {
  tokenizer_revision = "a3s.gateway.tokenizer.v1"
  expires_at = "2099-01-01T00:00:00Z"
}
"#;
        require_inference_tokenizer_revision(acl).unwrap();
    }

    #[test]
    fn require_rejects_missing_or_unknown_revision() {
        let missing = r#"
inference {
  expires_at = "2099-01-01T00:00:00Z"
}
"#;
        let error = require_inference_tokenizer_revision(missing).unwrap_err();
        assert!(error.contains("tokenizer_revision"));

        let wrong = r#"
inference {
  tokenizer_revision = "a3s.gateway.tokenizer.v0"
  expires_at = "2099-01-01T00:00:00Z"
}
"#;
        let error = require_inference_tokenizer_revision(wrong).unwrap_err();
        assert!(error.contains("a3s.gateway.tokenizer.v1"));
    }

    #[test]
    fn policy_shell_always_declares_frozen_tokenizer_revision() {
        let expires_at = Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap();
        let acl = render_inference_policy_shell_acl(expires_at).unwrap();
        require_inference_tokenizer_revision(&acl).unwrap();
        assert!(acl.contains("expires_at = \"2099-01-01T00:00:00.000Z\""));
        assert!(!acl.contains("credentials"));
        assert!(!acl.contains("routes"));
        assert!(!acl.contains("workers"));
    }
}
