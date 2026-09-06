use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workflow::domain::{
    AppendWorkflowAuthoringOperation, CreateWorkflowAuthoringJournal, IWorkflowAuthoringRepository,
    WorkflowAuthoringAppend, WorkflowAuthoringEntry, WorkflowAuthoringError,
    WorkflowAuthoringJournal, WorkflowAuthoringJournalKey, WorkflowAuthoringPage,
    WorkflowAuthoringSnapshot,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

/// Concurrency-safe reference adapter for the hosted authoring repository port.
///
/// This adapter is intended for application tests and local development. The
/// PostgreSQL adapter will retain the same domain transaction and CAS semantics.
#[derive(Default)]
pub struct InMemoryWorkflowAuthoringRepository {
    journals: RwLock<BTreeMap<WorkflowAuthoringJournalKey, WorkflowAuthoringJournal>>,
}

impl InMemoryWorkflowAuthoringRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IWorkflowAuthoringRepository for InMemoryWorkflowAuthoringRepository {
    async fn create(
        &self,
        write: CreateWorkflowAuthoringJournal,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        let mut journals = self.journals.write().await;
        if journals.contains_key(&write.key) {
            return Err(RepositoryError::Conflict(
                "workflow authoring journal already exists".into(),
            ));
        }
        let journal = WorkflowAuthoringJournal::try_new(write.initial_snapshot)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        journals.insert(write.key, journal.clone());
        Ok(journal)
    }

    async fn append(
        &self,
        write: AppendWorkflowAuthoringOperation,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        let mut journals = self.journals.write().await;
        let journal = journals
            .get_mut(&write.key)
            .ok_or(RepositoryError::NotFound)?;
        journal
            .append(write.operation, write.result_snapshot)
            .map_err(map_error)
    }

    async fn current_snapshot(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringSnapshot>, RepositoryError> {
        Ok(self
            .journals
            .read()
            .await
            .get(&key)
            .map(|journal| journal.current_snapshot().clone()))
    }

    async fn find_operation(
        &self,
        key: WorkflowAuthoringJournalKey,
        operation_id: &str,
    ) -> Result<Option<WorkflowAuthoringEntry>, RepositoryError> {
        Ok(self.journals.read().await.get(&key).and_then(|journal| {
            journal
                .entries()
                .iter()
                .find(|entry| entry.operation_id() == operation_id)
                .cloned()
        }))
    }

    async fn find(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringJournal>, RepositoryError> {
        Ok(self.journals.read().await.get(&key).cloned())
    }

    async fn page(
        &self,
        key: WorkflowAuthoringJournalKey,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<WorkflowAuthoringPage>, RepositoryError> {
        let journals = self.journals.read().await;
        let Some(journal) = journals.get(&key) else {
            return Ok(None);
        };
        journal
            .page(after_sequence, limit)
            .map(Some)
            .map_err(map_error)
    }
}

fn map_error(error: WorkflowAuthoringError) -> RepositoryError {
    match error {
        WorkflowAuthoringError::CasConflict { expected, actual } => {
            RepositoryError::Conflict(format!(
                "workflow authoring base digest mismatch: expected {expected}, received {actual}"
            ))
        }
        WorkflowAuthoringError::IdempotencyConflict { .. } => RepositoryError::IdempotencyConflict,
        other => RepositoryError::Storage(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, WorkflowDefinitionId};
    use crate::modules::workflow::domain::{WorkflowAuthoringOperation, WorkflowAuthoringSnapshot};
    use std::sync::Arc;

    fn key() -> WorkflowAuthoringJournalKey {
        WorkflowAuthoringJournalKey::new(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowDefinitionId::new(),
        )
    }

    fn snapshot(bytes: &[u8]) -> WorkflowAuthoringSnapshot {
        WorkflowAuthoringSnapshot::try_from_bytes(bytes.to_vec()).expect("snapshot")
    }

    fn operation(
        id: &str,
        base: &WorkflowAuthoringSnapshot,
        bytes: &[u8],
    ) -> WorkflowAuthoringOperation {
        WorkflowAuthoringOperation::try_new(id, base.snapshot_digest().clone(), bytes.to_vec())
            .expect("operation")
    }

    #[tokio::test]
    async fn create_append_find_and_page_preserve_the_domain_contract() {
        let repository = InMemoryWorkflowAuthoringRepository::new();
        let key = key();
        let initial = snapshot(b"initial");
        repository
            .create(CreateWorkflowAuthoringJournal {
                key,
                initial_snapshot: initial.clone(),
            })
            .await
            .expect("create");
        let result = repository
            .append(AppendWorkflowAuthoringOperation {
                key,
                operation: operation("op-1", &initial, b"opaque"),
                result_snapshot: snapshot(b"next"),
            })
            .await
            .expect("append");
        assert!(!result.replayed);
        assert_eq!(result.entry.sequence(), 1);

        let page = repository
            .page(key, None, 10)
            .await
            .expect("page")
            .expect("journal");
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0], result.entry);
        assert_eq!(
            repository
                .find(key)
                .await
                .expect("find")
                .expect("journal")
                .last_sequence(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_same_base_appends_have_one_winner() {
        let repository = Arc::new(InMemoryWorkflowAuthoringRepository::new());
        let key = key();
        let initial = snapshot(b"initial");
        repository
            .create(CreateWorkflowAuthoringJournal {
                key,
                initial_snapshot: initial.clone(),
            })
            .await
            .expect("create");

        let left = Arc::clone(&repository);
        let left_initial = initial.clone();
        let left_task = tokio::spawn(async move {
            left.append(AppendWorkflowAuthoringOperation {
                key,
                operation: operation("left", &left_initial, b"left"),
                result_snapshot: snapshot(b"left-result"),
            })
            .await
        });
        let right = Arc::clone(&repository);
        let right_task = tokio::spawn(async move {
            right
                .append(AppendWorkflowAuthoringOperation {
                    key,
                    operation: operation("right", &initial, b"right"),
                    result_snapshot: snapshot(b"right-result"),
                })
                .await
        });
        let left_result = left_task.await.expect("left task");
        let right_result = right_task.await.expect("right task");
        assert_ne!(left_result.is_ok(), right_result.is_ok());
        let conflict = [left_result.as_ref().err(), right_result.as_ref().err()]
            .into_iter()
            .flatten()
            .next();
        assert!(matches!(
            conflict,
            Some(RepositoryError::Conflict(message)) if message.contains("base digest")
        ));
        assert_eq!(
            repository
                .find(key)
                .await
                .expect("find")
                .expect("journal")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn duplicate_operations_replay_and_conflicting_ids_are_rejected() {
        let repository = InMemoryWorkflowAuthoringRepository::new();
        let key = key();
        let initial = snapshot(b"initial");
        repository
            .create(CreateWorkflowAuthoringJournal {
                key,
                initial_snapshot: initial.clone(),
            })
            .await
            .expect("create");
        let first = repository
            .append(AppendWorkflowAuthoringOperation {
                key,
                operation: operation("same", &initial, b"one"),
                result_snapshot: snapshot(b"next"),
            })
            .await
            .expect("append");
        let replay = repository
            .append(AppendWorkflowAuthoringOperation {
                key,
                operation: operation("same", &initial, b"one"),
                result_snapshot: snapshot(b"ignored"),
            })
            .await
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(replay.entry, first.entry);

        let conflict = repository
            .append(AppendWorkflowAuthoringOperation {
                key,
                operation: operation("same", &initial, b"different"),
                result_snapshot: snapshot(b"ignored"),
            })
            .await;
        assert!(matches!(
            conflict,
            Err(RepositoryError::IdempotencyConflict)
        ));
    }
}
