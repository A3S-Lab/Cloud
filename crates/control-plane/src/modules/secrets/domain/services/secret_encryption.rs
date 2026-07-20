use crate::modules::secrets::domain::EncryptedSecretValue;
use async_trait::async_trait;

#[async_trait]
pub trait ISecretEncryptionService: Send + Sync {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError>;

    async fn decrypt(
        &self,
        value: &EncryptedSecretValue,
        context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError>;

    async fn health(&self) -> Result<bool, SecretEncryptionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SecretEncryptionError {
    #[error("Secret encryption input is invalid: {0}")]
    InvalidInput(String),
    #[error("Secret encryption provider rejected the request: {0}")]
    Rejected(String),
    #[error("Secret encryption provider is unavailable: {0}")]
    Unavailable(String),
}
