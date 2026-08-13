use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn from_bytes(value: &[u8]) -> Self {
        Self(format!("sha256:{:x}", Sha256::digest(value)))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let Some(hexadecimal) = value.strip_prefix("sha256:") else {
            return Err("digest must use canonical sha256 syntax".into());
        };
        if hexadecimal.len() != 64
            || !hexadecimal
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("digest must use canonical sha256 syntax".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_lowercase_sha256_digests() {
        let value = format!("sha256:{}", "a".repeat(64));
        assert_eq!(Sha256Digest::parse(&value).expect("digest").as_str(), value);
        assert!(Sha256Digest::parse("a".repeat(64)).is_err());
        assert!(Sha256Digest::parse(format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(Sha256Digest::parse(format!("sha256:{}", "a".repeat(63))).is_err());
        assert_eq!(
            Sha256Digest::from_bytes(b"abc").as_str(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
