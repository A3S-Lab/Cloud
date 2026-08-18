use super::log_chunk_object::{
    prepare_log_object, validate_expected_checksum, validate_object_key, verify_log_object,
    MAX_LOG_OBJECT_BYTES,
};
use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use a3s_cloud_contracts::NodeLogChunkReport;
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LogChunkObjectStore {
    objects: ImmutableObjectClient,
}

impl LogChunkObjectStore {
    pub fn local(root: impl Into<PathBuf>) -> Result<Self, LogChunkStoreError> {
        let objects = ImmutableObjectClient::local(root, "logs").map_err(map_error)?;
        Ok(Self { objects })
    }

    pub(crate) fn from_client(objects: ImmutableObjectClient) -> Self {
        Self { objects }
    }
}

#[async_trait]
impl ILogChunkStore for LogChunkObjectStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        node_id: Uuid,
        _ordinal: u16,
        report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        let (object_key, body) = prepare_log_object(node_id, report)?;
        let write = self
            .objects
            .put(&object_key, body, MAX_LOG_OBJECT_BYTES)
            .await
            .map_err(map_error)?;
        Ok(StoredLogChunk {
            object_key,
            created: write.created,
        })
    }

    async fn get(
        &self,
        object_key: &str,
        expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        validate_object_key(object_key)?;
        validate_expected_checksum(expected_checksum)?;
        match self
            .objects
            .get(object_key, MAX_LOG_OBJECT_BYTES)
            .await
            .map_err(map_error)?
        {
            ImmutableObjectRead::Found(body) => verify_log_object(&body, expected_checksum),
            ImmutableObjectRead::Missing => Ok(RetrievedLogChunk::Missing),
            ImmutableObjectRead::Corrupt => Ok(RetrievedLogChunk::Corrupt),
        }
    }

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError> {
        validate_object_key(object_key)?;
        self.objects.remove(object_key).await.map_err(map_error)
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        self.objects.health().await.map_err(map_error)
    }
}

fn map_error(error: ImmutableObjectError) -> LogChunkStoreError {
    match error {
        ImmutableObjectError::Invalid(message) => LogChunkStoreError::Invalid(message),
        ImmutableObjectError::Conflict(object_key) => LogChunkStoreError::Conflict(object_key),
        ImmutableObjectError::Integrity(message) => LogChunkStoreError::Unavailable(message),
        ImmutableObjectError::Unsupported(message) => LogChunkStoreError::Unavailable(message),
        ImmutableObjectError::Unavailable(message) => LogChunkStoreError::Unavailable(message),
    }
}

#[cfg(test)]
#[path = "local_log_chunk_store_tests.rs"]
mod local_tests;

#[cfg(test)]
#[path = "s3_log_chunk_store_tests.rs"]
mod s3_tests;
