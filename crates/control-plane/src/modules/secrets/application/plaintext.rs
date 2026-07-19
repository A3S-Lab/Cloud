use sha2::{Digest, Sha256};
use zeroize::Zeroize;

#[derive(PartialEq, Eq)]
pub struct SecretPlaintext(Vec<u8>);

impl SecretPlaintext {
    pub fn new(value: impl Into<Vec<u8>>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 * 1024 {
            return Err("Secret value must contain between 1 byte and 1 MiB".into());
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn digest(&self) -> String {
        format!("sha256:{:x}", Sha256::digest(&self.0))
    }
}

impl std::fmt::Debug for SecretPlaintext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted-secret-plaintext>")
    }
}

impl Drop for SecretPlaintext {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_is_redacted() {
        let value = SecretPlaintext::new(b"do-not-log-this".to_vec()).expect("Secret value");
        let debug = format!("{value:?}");
        assert_eq!(debug, "<redacted-secret-plaintext>");
        assert!(!debug.contains("do-not-log-this"));
    }
}
