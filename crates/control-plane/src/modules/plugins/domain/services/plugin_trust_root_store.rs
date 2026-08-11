use crate::modules::plugins::domain::value_objects::PluginTrustRoot;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTrustRootWrite {
    pub replayed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginTrustRootStoreError {
    #[error("plugin trust-root request is invalid: {0}")]
    Invalid(String),
    #[error("plugin trust root was not found")]
    NotFound,
    #[error("plugin trust-root identity conflicts with stored content")]
    Conflict,
    #[error("plugin trust root failed integrity validation: {0}")]
    Integrity(String),
    #[error("plugin trust-root storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IPluginTrustRootStore: Send + Sync {
    async fn put(
        &self,
        root: &PluginTrustRoot,
        bytes: Vec<u8>,
    ) -> Result<PluginTrustRootWrite, PluginTrustRootStoreError>;

    async fn get(&self, root: &PluginTrustRoot) -> Result<Vec<u8>, PluginTrustRootStoreError>;
}
