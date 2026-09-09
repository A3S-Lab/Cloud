//! Frozen Edge tokenizer revision for Cloud-compiled inference ACL.
//!
//! Gateway fail-closes managed `inference` policy unless
//! `tokenizer_revision` equals this exact string. Cloud's future policy
//! compiler must emit the same attribute; this module is the shared contract
//! identity so Cloud and Gateway cannot silently diverge.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
