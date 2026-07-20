use crate::modules::shared_kernel::domain::{OrganizationId, SecretId};

#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedSecretValue {
    key_id: String,
    ciphertext: String,
}

impl EncryptedSecretValue {
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
                "encrypted Secret value must contain bounded single-line key and ciphertext".into(),
            );
        }
        Ok(())
    }
}

impl std::fmt::Debug for EncryptedSecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptedSecretValue")
            .field("key_id", &self.key_id)
            .field("ciphertext", &"<redacted-ciphertext>")
            .finish()
    }
}

pub fn secret_encryption_context(
    organization_id: OrganizationId,
    secret_id: SecretId,
    version: u64,
) -> Result<Vec<u8>, String> {
    if organization_id.as_uuid().is_nil() || secret_id.as_uuid().is_nil() || version == 0 {
        return Err("Secret encryption context identity is invalid".into());
    }
    let mut context = Vec::with_capacity(60);
    context.extend_from_slice(b"a3s.cloud.secret.v1\0");
    context.extend_from_slice(organization_id.as_uuid().as_bytes());
    context.extend_from_slice(secret_id.as_uuid().as_bytes());
    context.extend_from_slice(&version.to_be_bytes());
    Ok(context)
}
