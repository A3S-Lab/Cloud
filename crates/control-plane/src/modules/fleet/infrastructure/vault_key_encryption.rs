use crate::infrastructure::{VaultClient, VaultClientError};
use crate::modules::secrets::domain::{
    EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct VaultKeyEncryptionService {
    client: VaultClient,
    mount: String,
    key: String,
    key_id: String,
}

impl VaultKeyEncryptionService {
    pub fn new(
        address: &str,
        token: &str,
        mount: impl Into<String>,
        key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, SecretEncryptionError> {
        let mount = validate_segment("Vault Transit mount", mount.into())?;
        let key = validate_segment("Vault Transit key", key.into())?;
        let key_id = format!("vault:{mount}/{key}");
        Ok(Self {
            client: VaultClient::new(address, token, timeout).map_err(map_error)?,
            mount,
            key,
            key_id,
        })
    }
}

#[async_trait]
impl ISecretEncryptionService for VaultKeyEncryptionService {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
        validate_input(plaintext, context)?;
        let data: EncryptResponse = self
            .client
            .post(
                &format!("{}/encrypt/{}", self.mount, self.key),
                &EncryptRequest {
                    plaintext: STANDARD.encode(plaintext),
                    context: (!context.is_empty()).then(|| STANDARD.encode(context)),
                },
            )
            .await
            .map_err(map_error)?;
        EncryptedSecretValue::new(self.key_id.clone(), data.ciphertext)
            .map_err(SecretEncryptionError::Rejected)
    }

    async fn decrypt(
        &self,
        value: &EncryptedSecretValue,
        context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError> {
        if value.key_id() != self.key_id
            || value.ciphertext().is_empty()
            || value.ciphertext().len() > 2 * 1024 * 1024
            || context.len() > 16 * 1024
        {
            return Err(SecretEncryptionError::Rejected(
                "Vault encrypted value identity or bounds are invalid".into(),
            ));
        }
        let data: DecryptResponse = self
            .client
            .post(
                &format!("{}/decrypt/{}", self.mount, self.key),
                &DecryptRequest {
                    ciphertext: value.ciphertext(),
                    context: (!context.is_empty()).then(|| STANDARD.encode(context)),
                },
            )
            .await
            .map_err(map_error)?;
        STANDARD.decode(data.plaintext).map_err(|_| {
            SecretEncryptionError::Rejected("Vault returned invalid plaintext encoding".into())
        })
    }

    async fn health(&self) -> Result<bool, SecretEncryptionError> {
        self.client.health().await.map_err(map_error)
    }
}

#[derive(Serialize)]
struct EncryptRequest {
    plaintext: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}

#[derive(Deserialize)]
struct EncryptResponse {
    ciphertext: String,
}

#[derive(Serialize)]
struct DecryptRequest<'a> {
    ciphertext: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}

#[derive(Deserialize)]
struct DecryptResponse {
    plaintext: String,
}

fn validate_input(plaintext: &[u8], context: &[u8]) -> Result<(), SecretEncryptionError> {
    if plaintext.is_empty() || plaintext.len() > 1024 * 1024 || context.len() > 16 * 1024 {
        return Err(SecretEncryptionError::InvalidInput(
            "Vault encryption input exceeds protocol bounds".into(),
        ));
    }
    Ok(())
}

fn validate_segment(label: &str, value: String) -> Result<String, SecretEncryptionError> {
    if value.is_empty()
        || value.len() > 255
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(SecretEncryptionError::InvalidInput(format!(
            "{label} is invalid"
        )));
    }
    Ok(value)
}

fn map_error(error: VaultClientError) -> SecretEncryptionError {
    match error {
        VaultClientError::Configuration(message) => SecretEncryptionError::InvalidInput(message),
        VaultClientError::Rejected(message) => SecretEncryptionError::Rejected(message),
        VaultClientError::Unavailable(message) => SecretEncryptionError::Unavailable(message),
    }
}
