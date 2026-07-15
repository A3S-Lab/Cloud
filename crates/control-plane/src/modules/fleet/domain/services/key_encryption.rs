use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedValue {
    pub key_id: String,
    pub ciphertext: String,
}

#[async_trait]
pub trait IKeyEncryptionService: Send + Sync {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedValue, KeyEncryptionError>;

    async fn decrypt(
        &self,
        value: &EncryptedValue,
        context: &[u8],
    ) -> Result<Vec<u8>, KeyEncryptionError>;

    async fn health(&self) -> Result<bool, KeyEncryptionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum KeyEncryptionError {
    #[error("key encryption input is invalid: {0}")]
    InvalidInput(String),
    #[error("key encryption provider rejected the request: {0}")]
    Rejected(String),
    #[error("key encryption provider is unavailable: {0}")]
    Unavailable(String),
}
