use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::agents::domain::{
    AgentExecutionCheckpointObjectError, AgentExecutionCheckpointObjectInventoryEntry,
    AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectReference,
    AgentExecutionCheckpointObjectWrite, IAgentExecutionCheckpointObjectStore,
    MAX_AGENT_EXECUTION_CHECKPOINT_BYTES,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct AgentExecutionCheckpointObjectStore {
    objects: ImmutableObjectClient,
}

impl AgentExecutionCheckpointObjectStore {
    pub(crate) fn from_client(objects: ImmutableObjectClient) -> Self {
        Self { objects }
    }

    fn validate_body(
        reference: &AgentExecutionCheckpointObjectReference,
        body: &[u8],
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        if u64::try_from(body.len()).map_err(|_| {
            AgentExecutionCheckpointObjectError::Integrity(
                "checkpoint body length overflowed its committed representation".into(),
            )
        })? != reference.size_bytes
            || Sha256Digest::from_bytes(body) != reference.digest
        {
            return Err(AgentExecutionCheckpointObjectError::Integrity(
                "checkpoint bytes changed their committed digest or length".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IAgentExecutionCheckpointObjectStore for AgentExecutionCheckpointObjectStore {
    async fn put(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
        body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        Self::validate_body(reference, &body)?;
        let write = self
            .objects
            .put(
                &reference.object_ref,
                body,
                MAX_AGENT_EXECUTION_CHECKPOINT_BYTES as u64,
            )
            .await
            .map_err(map_object_error)?;
        Ok(AgentExecutionCheckpointObjectWrite {
            replayed: !write.created,
        })
    }

    async fn get(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        let body = match self
            .objects
            .get(
                &reference.object_ref,
                MAX_AGENT_EXECUTION_CHECKPOINT_BYTES as u64,
            )
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectRead::Found(body) => body,
            ImmutableObjectRead::Missing => {
                return Err(AgentExecutionCheckpointObjectError::NotFound)
            }
            ImmutableObjectRead::Corrupt => {
                return Err(AgentExecutionCheckpointObjectError::Integrity(
                    "stored checkpoint exceeds its object bound".into(),
                ))
            }
        };
        Self::validate_body(reference, &body)?;
        Ok(body)
    }

    async fn inventory_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectError>
    {
        let limit = u32::try_from(limit).map_err(|_| {
            AgentExecutionCheckpointObjectError::Invalid(
                "checkpoint object inventory limit overflowed".into(),
            )
        })?;
        let page = self
            .objects
            .list_page(None, after, limit)
            .await
            .map_err(map_object_error)?;
        Ok(AgentExecutionCheckpointObjectInventoryPage {
            entries: page
                .entries
                .into_iter()
                .map(|entry| AgentExecutionCheckpointObjectInventoryEntry {
                    object_ref: entry.key,
                    size_bytes: entry.size_bytes,
                })
                .collect(),
            next_after: page.next_after,
        })
    }

    async fn remove(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        self.objects
            .remove(&reference.object_ref)
            .await
            .map_err(map_object_error)
    }
}

fn map_object_error(error: ImmutableObjectError) -> AgentExecutionCheckpointObjectError {
    match error {
        ImmutableObjectError::Invalid(message) => {
            AgentExecutionCheckpointObjectError::Invalid(message)
        }
        ImmutableObjectError::Conflict(key) => AgentExecutionCheckpointObjectError::Conflict(key),
        ImmutableObjectError::Integrity(message) => {
            AgentExecutionCheckpointObjectError::Integrity(message)
        }
        ImmutableObjectError::Unsupported(message) | ImmutableObjectError::Unavailable(message) => {
            AgentExecutionCheckpointObjectError::Unavailable(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::DisposableS3TestContext;
    use crate::modules::agents::domain::{
        AgentExecutionCheckpointObjectReference, IAgentRepository,
        AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE, AGENT_EXECUTION_CHECKPOINT_NAMESPACE,
        AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA,
    };
    use crate::modules::agents::infrastructure::{
        AgentExecutionCheckpointObjectReconciler, InMemoryAgentRepository,
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use object_store::memory::InMemory;
    use std::sync::Arc;

    fn reference(bytes: &[u8]) -> AgentExecutionCheckpointObjectReference {
        let digest = Sha256Digest::from_bytes(bytes);
        let hexadecimal = digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("digest prefix");
        AgentExecutionCheckpointObjectReference {
            schema: AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA.into(),
            namespace: AGENT_EXECUTION_CHECKPOINT_NAMESPACE.into(),
            object_ref: format!(
                "organizations/{}/executions/{}/checkpoints/{}/sha256/{hexadecimal}/checkpoint.json",
                uuid::Uuid::now_v7(),
                uuid::Uuid::now_v7(),
                uuid::Uuid::now_v7(),
            ),
            digest,
            size_bytes: u64::try_from(bytes.len()).expect("checkpoint size"),
            media_type: AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE.into(),
        }
    }

    #[tokio::test]
    async fn checkpoint_objects_replay_exactly_and_fail_closed_on_tamper() {
        let directory = tempfile::tempdir().expect("checkpoint object directory");
        let client = ImmutableObjectClient::local(directory.path(), "agent-checkpoints")
            .expect("checkpoint object client");
        let store = AgentExecutionCheckpointObjectStore::from_client(client);
        let bytes = br#"{"schema":"checkpoint"}"#;
        let reference = reference(bytes);

        assert!(
            !store
                .put(&reference, bytes.to_vec())
                .await
                .expect("first checkpoint write")
                .replayed
        );
        assert!(
            store
                .put(&reference, bytes.to_vec())
                .await
                .expect("checkpoint replay")
                .replayed
        );
        assert_eq!(store.get(&reference).await.expect("checkpoint read"), bytes);
        assert!(matches!(
            store.put(&reference, b"tampered".to_vec()).await,
            Err(AgentExecutionCheckpointObjectError::Integrity(_))
        ));
    }

    #[tokio::test]
    async fn remote_checkpoint_inventory_is_paged_and_cleanup_is_idempotent() {
        let client =
            ImmutableObjectClient::from_store(Arc::new(InMemory::new()), "agent-checkpoints")
                .expect("checkpoint object client");
        let store = AgentExecutionCheckpointObjectStore::from_client(client);
        let bodies = [b"checkpoint-a".as_slice(), b"checkpoint-b", b"checkpoint-c"];
        let mut references = Vec::new();
        for body in bodies {
            let reference = reference(body);
            store
                .put(&reference, body.to_vec())
                .await
                .expect("checkpoint object write");
            references.push(reference);
        }
        references.sort_by(|left, right| left.object_ref.cmp(&right.object_ref));

        let first = store.inventory_page(None, 2).await.expect("first page");
        assert_eq!(first.entries.len(), 2);
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.object_ref.as_str())
                .collect::<Vec<_>>(),
            references[..2]
                .iter()
                .map(|reference| reference.object_ref.as_str())
                .collect::<Vec<_>>()
        );
        let second = store
            .inventory_page(first.next_after.as_deref(), 2)
            .await
            .expect("second page");
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].object_ref, references[2].object_ref);
        assert!(second.next_after.is_none());

        store.remove(&references[0]).await.expect("first cleanup");
        store.remove(&references[0]).await.expect("cleanup replay");
        assert!(matches!(
            store.get(&references[0]).await,
            Err(AgentExecutionCheckpointObjectError::NotFound)
        ));
    }

    #[tokio::test]
    #[ignore = "requires an explicitly configured disposable S3-compatible bucket"]
    async fn real_s3_compatible_checkpoint_orphan_reconciliation_is_exact_and_idempotent() {
        let context = DisposableS3TestContext::from_environment("agent-checkpoint-reconciliation")
            .expect("disposable S3 test context");
        let transport = if context.uses_secure_transport() {
            "https"
        } else {
            "http"
        };
        let store = Arc::new(AgentExecutionCheckpointObjectStore::from_client(
            context.client(),
        ));

        let result = exercise_real_s3_orphan_reconciliation(store).await;
        let cleanup = context.remove_all().await;
        if let Err(error) = result {
            panic!("real S3 checkpoint reconciliation failed: {error}; cleanup={cleanup:?}");
        }
        assert_eq!(
            cleanup.expect("clean disposable S3 checkpoint namespace"),
            0,
            "checkpoint reconciliation left objects for fixture cleanup"
        );
        println!(
            "A3S_CLOUD_A1_CHECKPOINT_S3_RECONCILIATION_CERTIFIED provider=s3-compatible transport={transport} orphan_inventory=1 orphan_cleanup=1 cleanup_fence=lease cleanup_replay=1 namespace_cleanup=verified"
        );
    }

    async fn exercise_real_s3_orphan_reconciliation(
        store: Arc<AgentExecutionCheckpointObjectStore>,
    ) -> Result<(), String> {
        let body = b"abandoned-real-s3-checkpoint".to_vec();
        let reference = reference(&body);
        let write = store
            .put(&reference, body)
            .await
            .map_err(|error| error.to_string())?;
        if write.replayed {
            return Err("first real S3 checkpoint write unexpectedly replayed".into());
        }

        let agents: Arc<dyn IAgentRepository> = Arc::new(InMemoryAgentRepository::new());
        let objects: Arc<dyn IAgentExecutionCheckpointObjectStore> = store.clone();
        let reconciler = AgentExecutionCheckpointObjectReconciler::new(
            agents,
            objects,
            std::time::Duration::from_secs(1),
            ChronoDuration::milliseconds(10),
            ChronoDuration::seconds(5),
            100,
        )?;
        let observed_at = Utc::now();
        let observed = reconciler
            .run_once_at(observed_at)
            .await
            .map_err(|error| error.to_string())?;
        if observed.inventoried != 1
            || observed.deferred != 1
            || observed.removed != 0
            || !observed.failures.is_empty()
        {
            return Err(format!(
                "real S3 checkpoint inventory did not defer the exact orphan: {observed:?}"
            ));
        }

        let cleaned = reconciler
            .run_once_at(observed_at + ChronoDuration::milliseconds(11))
            .await
            .map_err(|error| error.to_string())?;
        if cleaned.expired_claims != 1 || cleaned.removed != 1 || !cleaned.failures.is_empty() {
            return Err(format!(
                "real S3 checkpoint cleanup did not remove the exact leased orphan: {cleaned:?}"
            ));
        }
        if !matches!(
            store.get(&reference).await,
            Err(AgentExecutionCheckpointObjectError::NotFound)
        ) {
            return Err("real S3 checkpoint cleanup did not make the orphan absent".into());
        }

        let replay = reconciler
            .run_once_at(observed_at + ChronoDuration::milliseconds(12))
            .await
            .map_err(|error| error.to_string())?;
        if replay.expired_claims != 0
            || replay.inventoried != 0
            || replay.removed != 0
            || !replay.failures.is_empty()
        {
            return Err(format!(
                "real S3 checkpoint cleanup replay was not empty and idempotent: {replay:?}"
            ));
        }
        Ok(())
    }
}
