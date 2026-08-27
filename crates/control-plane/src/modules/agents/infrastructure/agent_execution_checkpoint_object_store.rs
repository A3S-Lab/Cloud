use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::agents::domain::{
    AgentExecutionCheckpointObjectError, AgentExecutionCheckpointObjectReference,
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
    use crate::modules::agents::domain::{
        AgentExecutionCheckpointObjectReference, AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE,
        AGENT_EXECUTION_CHECKPOINT_NAMESPACE, AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA,
    };

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
}
