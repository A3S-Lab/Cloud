use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone, PartialEq, Eq)]
pub struct EnrollmentTokenCredential {
    digest: String,
}

impl Serialize for EnrollmentTokenCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.digest)
    }
}

impl<'de> Deserialize<'de> for EnrollmentTokenCredential {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let digest = String::deserialize(deserializer)?;
        Self::from_digest(digest).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Debug for EnrollmentTokenCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnrollmentTokenCredential")
            .field("digest", &"[REDACTED]")
            .finish()
    }
}

impl EnrollmentTokenCredential {
    pub fn from_secret(secret: &str) -> Result<Self, String> {
        let Some(value) = secret.strip_prefix("a3sn_") else {
            return Err("enrollment token must use the a3sn_ prefix".into());
        };
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("enrollment token must contain 64 lowercase hexadecimal characters".into());
        }
        Ok(Self {
            digest: digest(secret),
        })
    }

    pub fn from_digest(digest: impl Into<String>) -> Result<Self, String> {
        let digest = digest.into();
        let Some(value) = digest.strip_prefix("sha256:") else {
            return Err("enrollment token digest must use sha256".into());
        };
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("enrollment token digest must contain 64 hexadecimal characters".into());
        }
        Ok(Self { digest })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn verify(&self, secret: &str) -> bool {
        let candidate = digest(secret);
        self.digest.as_bytes().ct_eq(candidate.as_bytes()).into()
    }
}

fn digest(secret: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(secret.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_redacts_one_time_secrets() {
        let secret = format!("a3sn_{}", "a".repeat(64));
        let credential = EnrollmentTokenCredential::from_secret(&secret).expect("credential");
        assert!(credential.verify(&secret));
        assert!(!credential.verify(&format!("a3sn_{}", "b".repeat(64))));
        assert!(!format!("{credential:?}").contains(&secret));
        assert!(EnrollmentTokenCredential::from_secret("a3sn_short").is_err());
    }
}
