use crate::infrastructure::{
    ImmutableObjectClient, ImmutableObjectError, ImmutableObjectVerification,
};
use crate::modules::files::application::{
    IUserFileObjectStore, UserFileObjectError, UserFileObjectReader,
};
use crate::modules::files::domain::{
    UserFileContentReference, UserFileObjectWrite, USER_FILE_MAX_BYTES,
};
use async_trait::async_trait;
use std::path::PathBuf;

pub const USER_FILE_OBJECT_NAMESPACE: &str = "user-files";

/// Files' typed adapter over the process-wide immutable-object client.
///
/// This adapter owns reference validation only. Provider construction and byte
/// durability remain with `ImmutableObjectClient` and its selected S0 backend.
#[derive(Debug, Clone)]
pub struct SharedUserFileObjectStore {
    objects: ImmutableObjectClient,
}

impl SharedUserFileObjectStore {
    pub fn local(root: impl Into<PathBuf>) -> Result<Self, UserFileObjectError> {
        let objects = ImmutableObjectClient::local(root, USER_FILE_OBJECT_NAMESPACE)
            .map_err(map_object_error)?;
        Ok(Self { objects })
    }

    pub(crate) fn from_client(objects: ImmutableObjectClient) -> Self {
        Self { objects }
    }
}

#[async_trait]
impl IUserFileObjectStore for SharedUserFileObjectStore {
    async fn put(
        &self,
        reference: &UserFileContentReference,
        reader: UserFileObjectReader,
    ) -> Result<UserFileObjectWrite, UserFileObjectError> {
        reference.validate().map_err(UserFileObjectError::Invalid)?;
        let key = reference
            .storage_key()
            .map_err(UserFileObjectError::Invalid)?;
        let write = self
            .objects
            .put_stream(
                &key,
                reader,
                reference.size_bytes,
                reference.digest.as_str(),
                USER_FILE_MAX_BYTES,
            )
            .await
            .map_err(map_object_error)?;
        Ok(UserFileObjectWrite::stored(
            reference.clone(),
            !write.created,
        ))
    }

    async fn verify(
        &self,
        reference: &UserFileContentReference,
    ) -> Result<(), UserFileObjectError> {
        reference.validate().map_err(UserFileObjectError::Invalid)?;
        let key = reference
            .storage_key()
            .map_err(UserFileObjectError::Invalid)?;
        match self
            .objects
            .verify(
                &key,
                reference.size_bytes,
                reference.digest.as_str(),
                USER_FILE_MAX_BYTES,
            )
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectVerification::Verified => Ok(()),
            ImmutableObjectVerification::Missing => Err(UserFileObjectError::NotFound),
            ImmutableObjectVerification::Corrupt => Err(UserFileObjectError::Integrity(
                "stored bytes do not match their immutable UserFile reference".into(),
            )),
        }
    }
}

fn map_object_error(error: ImmutableObjectError) -> UserFileObjectError {
    match error {
        ImmutableObjectError::Invalid(message) => UserFileObjectError::Invalid(message),
        ImmutableObjectError::Conflict(key) => UserFileObjectError::Conflict(key),
        ImmutableObjectError::Integrity(message) => UserFileObjectError::Integrity(message),
        ImmutableObjectError::Unsupported(message) | ImmutableObjectError::Unavailable(message) => {
            UserFileObjectError::Unavailable(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        OrganizationId, ProjectId, Sha256Digest, UserFileId, UserFileUploadId,
    };
    use std::io::Cursor;

    fn reader(bytes: &[u8]) -> UserFileObjectReader {
        Box::pin(Cursor::new(bytes.to_vec()))
    }

    fn reference(bytes: &[u8]) -> UserFileContentReference {
        UserFileContentReference::new(
            OrganizationId::new(),
            ProjectId::new(),
            UserFileId::new(),
            UserFileUploadId::new(),
            Sha256Digest::from_bytes(bytes),
            bytes.len() as u64,
            "application/octet-stream",
        )
        .expect("reference")
    }

    #[tokio::test]
    async fn exact_bytes_replay_and_verify_through_shared_client() {
        let directory = tempfile::tempdir().expect("UserFile object directory");
        let store = SharedUserFileObjectStore::local(directory.path()).expect("store");
        let bytes = b"admitted user content";
        let reference = reference(bytes);

        let first = store
            .put(&reference, reader(bytes))
            .await
            .expect("first write");
        assert!(!first.replayed());
        assert_eq!(first.reference(), &reference);
        let replay = store
            .put(&reference, reader(bytes))
            .await
            .expect("replay write");
        assert!(replay.replayed());
        store.verify(&reference).await.expect("verified read");
    }

    #[tokio::test]
    async fn mismatched_and_conflicting_bytes_fail_closed() {
        let directory = tempfile::tempdir().expect("UserFile object directory");
        let client = ImmutableObjectClient::local(directory.path(), USER_FILE_OBJECT_NAMESPACE)
            .expect("shared client");
        let store = SharedUserFileObjectStore::from_client(client.clone());
        let bytes = b"expected bytes";
        let reference = reference(bytes);

        assert!(matches!(
            store.put(&reference, reader(b"wrong bytes")).await,
            Err(UserFileObjectError::Integrity(_))
        ));
        assert_eq!(
            store.verify(&reference).await,
            Err(UserFileObjectError::NotFound)
        );
        client
            .put(
                &reference.storage_key().expect("key"),
                b"foreign bytes".to_vec(),
                USER_FILE_MAX_BYTES,
            )
            .await
            .expect("foreign write");
        assert!(matches!(
            store.put(&reference, reader(bytes)).await,
            Err(UserFileObjectError::Conflict(_) | UserFileObjectError::Integrity(_))
        ));
        assert!(matches!(
            store.verify(&reference).await,
            Err(UserFileObjectError::Integrity(_))
        ));
    }
}
