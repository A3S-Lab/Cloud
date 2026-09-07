use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPolicyWrite {
    pub replayed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginPolicyStoreError {
    #[error("plugin policy request is invalid: {0}")]
    Invalid(String),
    #[error("plugin policy was not found")]
    NotFound,
    #[error("plugin policy identity conflicts with stored content")]
    Conflict,
    #[error("plugin policy failed integrity validation: {0}")]
    Integrity(String),
    #[error("plugin policy storage failed: {0}")]
    Storage(String),
}

/// Content-addressed store for immutable plugin policy ACL bytes.
///
/// Assignments retain only the digest; authorize-trust loads the exact ACL
/// through this adapter before command-bound Node Artifact transfer.
#[async_trait]
pub trait IPluginPolicyStore: Send + Sync {
    async fn put(
        &self,
        digest: &Sha256Digest,
        bytes: Vec<u8>,
    ) -> Result<PluginPolicyWrite, PluginPolicyStoreError>;

    async fn get(&self, digest: &Sha256Digest) -> Result<Vec<u8>, PluginPolicyStoreError>;
}
