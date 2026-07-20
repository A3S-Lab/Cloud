use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogRetentionTarget {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub object_key: String,
    pub received_at: DateTime<Utc>,
}

impl NodeLogRetentionTarget {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.object_key.is_empty()
            || self.object_key.len() > 4096
            || self.object_key.contains('\0')
        {
            return Err("log retention target is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait ILogRetentionRepository: Send + Sync {
    async fn list_log_chunks_for_retention(
        &self,
        received_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<NodeLogRetentionTarget>, RepositoryError>;

    async fn mark_log_chunk_retained(
        &self,
        target: &NodeLogRetentionTarget,
        retained_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError>;
}
