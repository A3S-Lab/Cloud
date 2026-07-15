use a3s_cloud_contracts::NodeLogChunkReport;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLogChunk {
    pub object_key: String,
    pub created: bool,
}

#[async_trait]
pub trait ILogChunkStore: Send + Sync {
    async fn put(
        &self,
        batch_id: Uuid,
        node_id: Uuid,
        ordinal: u16,
        report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError>;

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError>;

    async fn health(&self) -> Result<bool, LogChunkStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum LogChunkStoreError {
    #[error("log chunk is invalid: {0}")]
    Invalid(String),
    #[error("log chunk object conflicts with existing content: {0}")]
    Conflict(String),
    #[error("log chunk store is unavailable: {0}")]
    Unavailable(String),
}
