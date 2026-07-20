use super::*;
use crate::modules::fleet::domain::repositories::{
    NodeLogCompactionResult, NodeLogRetentionTarget,
};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use tokio::sync::Mutex;

type CompactionCall = (DateTime<Utc>, DateTime<Utc>, usize);

#[derive(Default)]
struct CompactionRepository {
    calls: Mutex<Vec<CompactionCall>>,
}

#[async_trait]
impl ILogRetentionRepository for CompactionRepository {
    async fn list_log_chunks_for_retention(
        &self,
        _received_before: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<NodeLogRetentionTarget>, RepositoryError> {
        Err(RepositoryError::Storage(
            "unexpected body-retention scan".into(),
        ))
    }

    async fn mark_log_chunk_retained(
        &self,
        _target: &NodeLogRetentionTarget,
        _retained_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        Err(RepositoryError::Storage(
            "unexpected body-retention commit".into(),
        ))
    }

    async fn compact_log_tombstones(
        &self,
        retained_before: DateTime<Utc>,
        compacted_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<NodeLogCompactionResult, RepositoryError> {
        self.calls
            .lock()
            .await
            .push((retained_before, compacted_at, limit));
        Ok(NodeLogCompactionResult {
            compacted_tombstones: limit,
            created_ranges: 1,
        })
    }
}

#[tokio::test]
async fn compaction_uses_an_independent_age_and_bounded_batch() {
    let repository = Arc::new(CompactionRepository::default());
    let worker = LogCompactionWorker::new(
        repository.clone(),
        Duration::from_secs(300),
        Duration::from_secs(30),
        128,
    )
    .expect("compaction worker");
    let now = Utc::now();

    let result = worker.run_once(now).await.expect("compaction cycle");

    assert_eq!(
        result,
        NodeLogCompactionResult {
            compacted_tombstones: 128,
            created_ranges: 1,
        }
    );
    assert_eq!(
        repository.calls.lock().await.as_slice(),
        &[(now - ChronoDuration::minutes(5), now, 128)]
    );
}

#[test]
fn compaction_rejects_unbounded_or_inverted_policy() {
    let repository: Arc<dyn ILogRetentionRepository> = Arc::new(CompactionRepository::default());
    assert!(LogCompactionWorker::new(
        repository.clone(),
        Duration::from_secs(60),
        Duration::from_secs(61),
        1,
    )
    .is_err());
    assert!(LogCompactionWorker::new(
        repository,
        Duration::from_secs(60),
        Duration::from_secs(1),
        10_001,
    )
    .is_err());
}
