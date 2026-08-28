use super::Sha256Digest;
use serde::{Deserialize, Serialize};

const MAX_AUTHORIZATION_DECISION_ID_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationDecisionRef {
    pub id: String,
    pub digest: Sha256Digest,
}

/// Neutral name used when a later authorization decision binds an earlier
/// authentication or authorization proof. This is an alias, not a second
/// evidence-reference representation.
pub type DecisionEvidenceRef = AuthorizationDecisionRef;

impl AuthorizationDecisionRef {
    pub fn new(id: impl Into<String>, digest: Sha256Digest) -> Result<Self, String> {
        let value = Self {
            id: id.into(),
            digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        Sha256Digest::parse(self.digest.as_str())?;
        if self.id.is_empty()
            || self.id.trim() != self.id
            || self.id.len() > MAX_AUTHORIZATION_DECISION_ID_BYTES
            || self.id.contains(['\0', '\r', '\n'])
        {
            return Err("authorization decision identity is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest")
    }

    #[test]
    fn validates_bounded_exact_authorization_decisions() {
        AuthorizationDecisionRef::new("grant-evaluation-1", digest()).expect("reference");
        assert!(AuthorizationDecisionRef::new("", digest()).is_err());
        assert!(AuthorizationDecisionRef::new(" padded ", digest()).is_err());
        assert!(AuthorizationDecisionRef::new("line\nbreak", digest()).is_err());

        let forged: AuthorizationDecisionRef =
            serde_json::from_str(r#"{"id":"grant-evaluation-1","digest":"sha256:not-a-digest"}"#)
                .expect("wire shape");
        assert!(forged.validate().is_err());
    }
}
