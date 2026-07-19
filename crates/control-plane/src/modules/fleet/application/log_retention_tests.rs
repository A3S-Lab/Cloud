use super::*;
use crate::modules::fleet::domain::repositories::NodeLogCompactionResult;
use crate::modules::fleet::domain::services::{
    LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use crate::modules::shared_kernel::domain::NodeId;
use a3s_cloud_contracts::NodeLogChunkReport;
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use uuid::Uuid;

struct RetentionRepository {
    targets: Mutex<Vec<NodeLogRetentionTarget>>,
    mark_failures: AtomicUsize,
    marks: Mutex<Vec<NodeLogRetentionTarget>>,
}

#[async_trait]
impl ILogRetentionRepository for RetentionRepository {
    async fn list_log_chunks_for_retention(
        &self,
        received_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<NodeLogRetentionTarget>, RepositoryError> {
        Ok(self
            .targets
            .lock()
            .await
            .iter()
            .filter(|target| target.received_at < received_before)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn mark_log_chunk_retained(
        &self,
        target: &NodeLogRetentionTarget,
        _retained_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        if self.mark_failures.load(Ordering::SeqCst) > 0 {
            self.mark_failures.fetch_sub(1, Ordering::SeqCst);
            return Err(RepositoryError::Storage(
                "injected retention commit interruption".into(),
            ));
        }
        let mut targets = self.targets.lock().await;
        let Some(index) = targets.iter().position(|candidate| candidate == target) else {
            return Ok(false);
        };
        self.marks.lock().await.push(target.clone());
        targets.remove(index);
        Ok(true)
    }

    async fn compact_log_tombstones(
        &self,
        _retained_before: DateTime<Utc>,
        _compacted_at: DateTime<Utc>,
        _limit: usize,
    ) -> Result<NodeLogCompactionResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "unexpected tombstone compaction".into(),
        ))
    }
}

struct RetentionObjectStore {
    remove_failures: AtomicUsize,
    removals: Mutex<Vec<String>>,
}

#[async_trait]
impl ILogChunkStore for RetentionObjectStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        _node_id: Uuid,
        _ordinal: u16,
        _report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        Err(LogChunkStoreError::Unavailable(
            "unexpected retention put".into(),
        ))
    }

    async fn get(
        &self,
        _object_key: &str,
        _expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        Err(LogChunkStoreError::Unavailable(
            "unexpected retention get".into(),
        ))
    }

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError> {
        self.removals.lock().await.push(object_key.into());
        if self.remove_failures.load(Ordering::SeqCst) > 0 {
            self.remove_failures.fetch_sub(1, Ordering::SeqCst);
            return Err(LogChunkStoreError::Unavailable(
                "injected object deletion interruption".into(),
            ));
        }
        Ok(())
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        Ok(true)
    }
}

#[tokio::test]
async fn object_deletion_failure_keeps_metadata_for_a_later_retry() {
    let now = Utc::now();
    let target = target(now - ChronoDuration::minutes(2));
    let repository = Arc::new(RetentionRepository {
        targets: Mutex::new(vec![target.clone()]),
        mark_failures: AtomicUsize::new(0),
        marks: Mutex::new(Vec::new()),
    });
    let objects = Arc::new(RetentionObjectStore {
        remove_failures: AtomicUsize::new(1),
        removals: Mutex::new(Vec::new()),
    });
    let worker = worker(repository.clone(), objects.clone());

    let interrupted = worker.run_once(now).await.expect("retention scan");
    assert_eq!(interrupted.inspected, 1);
    assert_eq!(interrupted.retained, 0);
    assert_eq!(interrupted.failures.len(), 1);
    assert!(repository.marks.lock().await.is_empty());

    let recovered = worker
        .run_once(now + ChronoDuration::seconds(1))
        .await
        .expect("retention retry");
    assert_eq!(recovered.retained, 1);
    assert!(recovered.failures.is_empty());
    assert_eq!(repository.marks.lock().await.as_slice(), &[target]);
    assert_eq!(objects.removals.lock().await.len(), 2);
}

#[tokio::test]
async fn metadata_commit_failure_repeats_the_idempotent_object_delete() {
    let now = Utc::now();
    let target = target(now - ChronoDuration::minutes(2));
    let repository = Arc::new(RetentionRepository {
        targets: Mutex::new(vec![target.clone()]),
        mark_failures: AtomicUsize::new(1),
        marks: Mutex::new(Vec::new()),
    });
    let objects = Arc::new(RetentionObjectStore {
        remove_failures: AtomicUsize::new(0),
        removals: Mutex::new(Vec::new()),
    });
    let worker = worker(repository.clone(), objects.clone());

    let interrupted = worker.run_once(now).await.expect("retention scan");
    assert_eq!(interrupted.retained, 0);
    assert_eq!(interrupted.failures.len(), 1);
    assert!(repository.marks.lock().await.is_empty());

    let recovered = worker
        .run_once(now + ChronoDuration::seconds(1))
        .await
        .expect("retention retry");
    assert_eq!(recovered.retained, 1);
    assert_eq!(repository.marks.lock().await.as_slice(), &[target]);
    assert_eq!(objects.removals.lock().await.len(), 2);
}

fn worker(
    repository: Arc<RetentionRepository>,
    objects: Arc<RetentionObjectStore>,
) -> LogRetentionWorker {
    LogRetentionWorker::new(
        repository,
        objects,
        Duration::from_secs(60),
        Duration::from_secs(1),
        16,
    )
    .expect("retention worker")
}

fn target(received_at: DateTime<Utc>) -> NodeLogRetentionTarget {
    NodeLogRetentionTarget {
        node_id: NodeId::new(),
        unit_id: "service-1".into(),
        generation: 1,
        sequence: 1,
        object_key: "nodes/node/units/unit/generations/1/chunks/1.json".into(),
        received_at,
    }
}
