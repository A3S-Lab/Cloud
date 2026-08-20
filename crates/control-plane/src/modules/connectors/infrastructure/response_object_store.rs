use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::connectors::domain::{
    ConnectorResponseObjectError, ConnectorResponseObjectReference, ConnectorResponseObjectWrite,
    IConnectorResponseObjectStore, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use std::path::PathBuf;

pub const CONNECTOR_RESPONSE_OBJECT_NAMESPACE: &str = "connector-responses";

#[derive(Debug, Clone)]
pub struct ConnectorResponseObjectStore {
    objects: ImmutableObjectClient,
}

impl ConnectorResponseObjectStore {
    pub fn local(root: impl Into<PathBuf>) -> Result<Self, ConnectorResponseObjectError> {
        let objects = ImmutableObjectClient::local(root, CONNECTOR_RESPONSE_OBJECT_NAMESPACE)
            .map_err(map_object_error)?;
        Ok(Self { objects })
    }

    pub(crate) fn from_client(objects: ImmutableObjectClient) -> Self {
        Self { objects }
    }

    fn validate_body(
        reference: &ConnectorResponseObjectReference,
        body: &[u8],
    ) -> Result<(), ConnectorResponseObjectError> {
        reference
            .validate()
            .map_err(ConnectorResponseObjectError::Invalid)?;
        if body.len() as u64 != reference.size_bytes
            || Sha256Digest::from_bytes(body) != reference.digest
        {
            return Err(ConnectorResponseObjectError::Integrity(
                "response bytes do not match their immutable reference".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IConnectorResponseObjectStore for ConnectorResponseObjectStore {
    async fn put(
        &self,
        reference: &ConnectorResponseObjectReference,
        body: Vec<u8>,
    ) -> Result<ConnectorResponseObjectWrite, ConnectorResponseObjectError> {
        Self::validate_body(reference, &body)?;
        let key = reference
            .storage_key()
            .map_err(ConnectorResponseObjectError::Invalid)?;
        let write = self
            .objects
            .put(&key, body, MAXIMUM_CONNECTOR_BODY_BYTES as u64)
            .await
            .map_err(map_object_error)?;
        Ok(ConnectorResponseObjectWrite {
            replayed: !write.created,
        })
    }

    async fn get(
        &self,
        reference: &ConnectorResponseObjectReference,
    ) -> Result<Vec<u8>, ConnectorResponseObjectError> {
        reference
            .validate()
            .map_err(ConnectorResponseObjectError::Invalid)?;
        let key = reference
            .storage_key()
            .map_err(ConnectorResponseObjectError::Invalid)?;
        let body = match self
            .objects
            .get(&key, MAXIMUM_CONNECTOR_BODY_BYTES as u64)
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectRead::Found(body) => body,
            ImmutableObjectRead::Missing => return Err(ConnectorResponseObjectError::NotFound),
            ImmutableObjectRead::Corrupt => {
                return Err(ConnectorResponseObjectError::Integrity(
                    "stored response exceeds the Connector body bound".into(),
                ))
            }
        };
        Self::validate_body(reference, &body)?;
        Ok(body)
    }
}

fn map_object_error(error: ImmutableObjectError) -> ConnectorResponseObjectError {
    match error {
        ImmutableObjectError::Invalid(message) => ConnectorResponseObjectError::Invalid(message),
        ImmutableObjectError::Conflict(key) => ConnectorResponseObjectError::Conflict(key),
        ImmutableObjectError::Integrity(message) => {
            ConnectorResponseObjectError::Integrity(message)
        }
        ImmutableObjectError::Unsupported(message) | ImmutableObjectError::Unavailable(message) => {
            ConnectorResponseObjectError::Unavailable(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::ConnectorResponseObjectReference;
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    };
    use uuid::Uuid;

    fn reference(bytes: &[u8]) -> ConnectorResponseObjectReference {
        ConnectorResponseObjectReference::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            Uuid::now_v7(),
            Sha256Digest::from_bytes(bytes),
            bytes.len() as u64,
        )
        .expect("reference")
    }

    #[tokio::test]
    async fn exact_response_replays_and_reads_from_the_shared_client() {
        let directory = tempfile::tempdir().expect("response object directory");
        let store = ConnectorResponseObjectStore::local(directory.path()).expect("store");
        let bytes = br#"{"accepted":true}"#;
        let reference = reference(bytes);

        let first = store
            .put(&reference, bytes.to_vec())
            .await
            .expect("first write");
        assert!(!first.replayed);
        let replay = store
            .put(&reference, bytes.to_vec())
            .await
            .expect("replay write");
        assert!(replay.replayed);
        assert_eq!(store.get(&reference).await.expect("read"), bytes);
    }

    #[tokio::test]
    async fn digest_size_missing_and_conflicting_content_fail_closed() {
        let directory = tempfile::tempdir().expect("response object directory");
        let store = ConnectorResponseObjectStore::local(directory.path()).expect("store");
        let bytes = b"accepted";
        let reference = reference(bytes);

        assert!(matches!(
            store.put(&reference, b"tampered".to_vec()).await,
            Err(ConnectorResponseObjectError::Integrity(_))
        ));
        assert!(matches!(
            store.get(&reference).await,
            Err(ConnectorResponseObjectError::NotFound)
        ));
        store
            .put(&reference, bytes.to_vec())
            .await
            .expect("stored response");

        let mut foreign = reference.clone();
        foreign.digest = Sha256Digest::from_bytes(b"different");
        assert!(matches!(
            store.get(&foreign).await,
            Err(ConnectorResponseObjectError::Invalid(_))
        ));
    }
}
