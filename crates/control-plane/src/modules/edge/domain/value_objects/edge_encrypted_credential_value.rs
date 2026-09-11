/// Edge-owned encrypted MCP credential delivery material.
///
/// Secrets remains authoritative for key material and ciphertext encoding.
/// This value object carries only the bounded key/ciphertext pair Edge needs
/// to persist and recover short-lived delivery receipts.
#[derive(Clone, PartialEq, Eq)]
pub struct EdgeEncryptedCredentialValue {
    key_id: String,
    ciphertext: String,
}

impl EdgeEncryptedCredentialValue {
    pub fn new(key_id: impl Into<String>, ciphertext: impl Into<String>) -> Result<Self, String> {
        let value = Self {
            key_id: key_id.into(),
            ciphertext: ciphertext.into(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn ciphertext(&self) -> &str {
        &self.ciphertext
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.key_id.trim() != self.key_id
            || self.key_id.is_empty()
            || self.key_id.len() > 512
            || self.key_id.contains(['\0', '\r', '\n'])
            || self.ciphertext.trim() != self.ciphertext
            || self.ciphertext.is_empty()
            || self.ciphertext.len() > 2 * 1024 * 1024
            || self.ciphertext.contains(['\0', '\r', '\n'])
        {
            return Err(
                "encrypted MCP credential delivery value must contain bounded single-line key and ciphertext"
                    .into(),
            );
        }
        Ok(())
    }
}

impl std::fmt::Debug for EdgeEncryptedCredentialValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EdgeEncryptedCredentialValue")
            .field("key_id", &self.key_id)
            .field("ciphertext", &"<redacted-ciphertext>")
            .finish()
    }
}
