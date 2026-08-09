use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

const MAX_ASSIGNMENT_POLICY_ID_BYTES: usize = 512;
const MAX_PORTABLE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentPolicyRef {
    pub id: String,
    pub revision: u64,
    pub digest: Sha256Digest,
}

impl AssignmentPolicyRef {
    pub fn new(id: impl Into<String>, revision: u64, digest: Sha256Digest) -> Result<Self, String> {
        let value = Self {
            id: id.into(),
            revision,
            digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.trim() != self.id
            || self.id.len() > MAX_ASSIGNMENT_POLICY_ID_BYTES
            || self.id.contains(['\0', '\r', '\n'])
            || self.revision == 0
            || self.revision > MAX_PORTABLE_INTEGER
        {
            return Err("assignment policy reference is invalid".into());
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
    fn requires_an_exact_positive_policy_revision() {
        AssignmentPolicyRef::new("approval-policy", 1, digest()).expect("reference");
        assert!(AssignmentPolicyRef::new("approval-policy", 0, digest()).is_err());
        assert!(AssignmentPolicyRef::new(" padded ", 1, digest()).is_err());
    }
}
