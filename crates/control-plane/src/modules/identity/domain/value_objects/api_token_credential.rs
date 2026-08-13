use sha2::{Digest, Sha256};
use std::fmt::Write;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

#[derive(Clone, PartialEq, Eq)]
pub struct ApiTokenSecret(Zeroizing<String>);

impl ApiTokenSecret {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let encoded = value.strip_prefix("a3s_").unwrap_or_default();
        if encoded.len() != 64
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(
                "API token must use the a3s_ prefix followed by 64 lowercase hex digits".into(),
            );
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub fn digest(&self) -> ApiTokenDigest {
        ApiTokenDigest(format!("sha256:{:x}", Sha256::digest(self.0.as_bytes())))
    }

    pub fn generate() -> Result<(Zeroizing<String>, ApiTokenDigest), String> {
        let mut random = Zeroizing::new([0_u8; 32]);
        getrandom::fill(&mut *random)
            .map_err(|error| format!("could not generate API token: {error}"))?;
        let mut plaintext = Zeroizing::new(String::with_capacity(68));
        plaintext.push_str("a3s_");
        for byte in random.iter() {
            write!(&mut plaintext, "{byte:02x}")
                .map_err(|error| format!("could not encode API token: {error}"))?;
        }
        let digest = Self(plaintext.clone()).digest();
        Ok((plaintext, digest))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenDigest(String);

impl ApiTokenDigest {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let encoded = value.strip_prefix("sha256:").unwrap_or_default();
        if encoded.len() != 64
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("stored API token digest is invalid".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct BootstrapCredential([u8; 32]);

impl BootstrapCredential {
    pub fn new(secret: &str) -> Result<Self, String> {
        if secret.len() < 32 || secret.len() > 512 || secret.contains(['\0', '\r', '\n']) {
            return Err("bootstrap credential must contain 32 to 512 safe bytes".into());
        }
        Ok(Self(Sha256::digest(secret.as_bytes()).into()))
    }

    pub fn verify(&self, candidate: &str) -> bool {
        let candidate: [u8; 32] = Sha256::digest(candidate.as_bytes()).into();
        bool::from(self.0.ct_eq(&candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_validated_and_compared_without_plaintext_storage() {
        let token = ApiTokenSecret::parse(format!("a3s_{}", "a".repeat(64))).expect("token");
        assert!(token.digest().as_str().starts_with("sha256:"));
        assert!(ApiTokenSecret::parse("short").is_err());
        let (generated, digest) = ApiTokenSecret::generate().expect("generated token");
        assert!(ApiTokenSecret::parse(generated.to_string()).is_ok());
        assert_eq!(
            ApiTokenSecret::parse(generated.to_string())
                .expect("generated token")
                .digest(),
            digest
        );

        let bootstrap = BootstrapCredential::new(&"b".repeat(32)).expect("bootstrap");
        assert!(bootstrap.verify(&"b".repeat(32)));
        assert!(!bootstrap.verify(&"c".repeat(32)));
    }
}
