use super::SecretId;
use serde::{Deserialize, Serialize};

const MAX_SAFE_SERIALIZED_INTEGER: u64 = 9_007_199_254_740_991;

/// Exact Secrets-owned version identity used by immutable bindings.
///
/// This value never carries plaintext and never means "latest". Owning
/// application services must still ask Secrets to authorize and materialize
/// the exact tenant-scoped version immediately before use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretVersionReference {
    pub secret_id: SecretId,
    pub version: u64,
}

impl SecretVersionReference {
    pub fn new(secret_id: SecretId, version: u64) -> Result<Self, String> {
        let reference = Self { secret_id, version };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.secret_id.as_uuid().is_nil()
            || self.version == 0
            || self.version > MAX_SAFE_SERIALIZED_INTEGER
        {
            return Err("Secret version reference is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn exact_reference_rejects_nil_latest_and_unsafe_integer_aliases() {
        SecretVersionReference::new(SecretId::new(), 1).expect("exact reference");
        assert!(SecretVersionReference::new(SecretId::from_uuid(Uuid::nil()), 1).is_err());
        assert!(SecretVersionReference::new(SecretId::new(), 0).is_err());
        assert!(
            SecretVersionReference::new(SecretId::new(), MAX_SAFE_SERIALIZED_INTEGER + 1).is_err()
        );
    }
}
