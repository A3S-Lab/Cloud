use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::plugins::domain::services::{
    IPluginTrustRootStore, PluginTrustRootStoreError, PluginTrustRootWrite,
};
use crate::modules::plugins::domain::value_objects::PluginTrustRoot;
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const OBJECT_NAMESPACE: &str = "plugin-trust-roots";

#[derive(Debug, Clone)]
pub struct PluginTrustRootObjectStore {
    objects: ImmutableObjectClient,
    maximum_root_bytes: u64,
}

impl PluginTrustRootObjectStore {
    /// `maximum_root_bytes` is injected by host composition from the owning
    /// A3S Use Registry contract. Cloud deliberately defines no second bound.
    pub fn local(
        root: impl Into<PathBuf>,
        maximum_root_bytes: u64,
    ) -> Result<Self, PluginTrustRootStoreError> {
        let objects =
            ImmutableObjectClient::local(root, OBJECT_NAMESPACE).map_err(map_object_error)?;
        Self::from_client(objects, maximum_root_bytes)
    }

    pub(crate) fn from_client(
        objects: ImmutableObjectClient,
        maximum_root_bytes: u64,
    ) -> Result<Self, PluginTrustRootStoreError> {
        if maximum_root_bytes == 0 {
            return Err(PluginTrustRootStoreError::Invalid(
                "plugin trust-root size bound must be positive".into(),
            ));
        }
        Ok(Self {
            objects,
            maximum_root_bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory(maximum_root_bytes: u64) -> Result<Self, PluginTrustRootStoreError> {
        let objects: std::sync::Arc<dyn object_store::ObjectStore> =
            std::sync::Arc::new(object_store::memory::InMemory::new());
        let client = ImmutableObjectClient::from_store(objects, OBJECT_NAMESPACE)
            .map_err(map_object_error)?;
        Self::from_client(client, maximum_root_bytes)
    }

    fn validate_bytes(
        &self,
        root: &PluginTrustRoot,
        bytes: &[u8],
    ) -> Result<(), PluginTrustRootStoreError> {
        root.validate()
            .map_err(PluginTrustRootStoreError::Invalid)?;
        if bytes.is_empty() || bytes.len() as u64 > self.maximum_root_bytes {
            return Err(PluginTrustRootStoreError::Invalid(
                "plugin trust-root bytes are empty or exceed the configured bound".into(),
            ));
        }
        let digest = Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(bytes)))
            .map_err(PluginTrustRootStoreError::Integrity)?;
        if digest != *root.digest() {
            return Err(PluginTrustRootStoreError::Integrity(
                "plugin trust-root bytes do not match their content address".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IPluginTrustRootStore for PluginTrustRootObjectStore {
    async fn put(
        &self,
        root: &PluginTrustRoot,
        bytes: Vec<u8>,
    ) -> Result<PluginTrustRootWrite, PluginTrustRootStoreError> {
        self.validate_bytes(root, &bytes)?;
        let write = self
            .objects
            .put(root.object_ref().as_str(), bytes, self.maximum_root_bytes)
            .await
            .map_err(map_object_error)?;
        Ok(PluginTrustRootWrite {
            replayed: !write.created,
        })
    }

    async fn get(&self, root: &PluginTrustRoot) -> Result<Vec<u8>, PluginTrustRootStoreError> {
        root.validate()
            .map_err(PluginTrustRootStoreError::Invalid)?;
        let bytes = match self
            .objects
            .get(root.object_ref().as_str(), self.maximum_root_bytes)
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectRead::Found(bytes) => bytes,
            ImmutableObjectRead::Missing => return Err(PluginTrustRootStoreError::NotFound),
            ImmutableObjectRead::Corrupt => {
                return Err(PluginTrustRootStoreError::Integrity(
                    "stored plugin trust root exceeds its admission bound".into(),
                ))
            }
        };
        self.validate_bytes(root, &bytes)?;
        Ok(bytes)
    }
}

fn map_object_error(error: ImmutableObjectError) -> PluginTrustRootStoreError {
    match error {
        ImmutableObjectError::Invalid(message) => PluginTrustRootStoreError::Invalid(message),
        ImmutableObjectError::Conflict(_) => PluginTrustRootStoreError::Conflict,
        ImmutableObjectError::Integrity(message) => PluginTrustRootStoreError::Integrity(message),
        ImmutableObjectError::Unsupported(message) => PluginTrustRootStoreError::Storage(message),
        ImmutableObjectError::Unavailable(message) => PluginTrustRootStoreError::Storage(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::plugins::domain::value_objects::PluginTrustRoot;
    use object_store::memory::InMemory;
    use object_store::path::Path as ObjectPath;
    use object_store::ObjectStore;
    use std::sync::Arc;

    fn trust_root(bytes: &[u8]) -> PluginTrustRoot {
        let digest = Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(bytes)))
            .expect("root digest");
        PluginTrustRoot::from_digest(digest, 1).expect("trust root")
    }

    fn object_store(
        objects: Arc<dyn ObjectStore>,
        maximum_root_bytes: u64,
    ) -> PluginTrustRootObjectStore {
        let client = ImmutableObjectClient::from_store(objects, OBJECT_NAMESPACE)
            .expect("immutable object client");
        PluginTrustRootObjectStore::from_client(client, maximum_root_bytes)
            .expect("plugin trust-root store")
    }

    #[tokio::test]
    async fn exact_content_addressed_root_replays_and_reads() {
        let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = object_store(objects, 1024);
        let bytes = b"signed root metadata";
        let root = trust_root(bytes);

        let first = store
            .put(&root, bytes.to_vec())
            .await
            .expect("first root write");
        assert!(!first.replayed);
        let replay = store.put(&root, bytes.to_vec()).await.expect("root replay");
        assert!(replay.replayed);
        assert_eq!(store.get(&root).await.expect("stored root"), bytes);
    }

    #[tokio::test]
    async fn invalid_digest_empty_and_oversized_roots_fail_before_storage() {
        let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = object_store(objects, 8);
        let root = trust_root(b"expected");

        assert!(matches!(
            store.put(&root, b"changed!".to_vec()).await,
            Err(PluginTrustRootStoreError::Integrity(_))
        ));
        assert!(matches!(
            store.put(&root, Vec::new()).await,
            Err(PluginTrustRootStoreError::Invalid(_))
        ));
        assert!(matches!(
            store.put(&root, b"oversized".to_vec()).await,
            Err(PluginTrustRootStoreError::Invalid(_))
        ));
        assert!(matches!(
            store.get(&root).await,
            Err(PluginTrustRootStoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn stored_tampering_and_conflicting_repair_fail_closed() {
        let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let store = object_store(Arc::clone(&objects), 1024);
        let bytes = b"signed root metadata";
        let root = trust_root(bytes);
        store.put(&root, bytes.to_vec()).await.expect("stored root");

        let path = ObjectPath::parse(format!("{OBJECT_NAMESPACE}/{}", root.object_ref().as_str()))
            .expect("root object path");
        objects
            .put(&path, b"tampered root metadata".as_slice().into())
            .await
            .expect("tamper stored root");

        assert!(matches!(
            store.get(&root).await,
            Err(PluginTrustRootStoreError::Integrity(_))
        ));
        assert!(matches!(
            store.put(&root, bytes.to_vec()).await,
            Err(PluginTrustRootStoreError::Conflict)
        ));
    }

    #[test]
    fn zero_size_bound_is_rejected() {
        let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let client = ImmutableObjectClient::from_store(objects, OBJECT_NAMESPACE)
            .expect("immutable object client");
        assert!(matches!(
            PluginTrustRootObjectStore::from_client(client, 0),
            Err(PluginTrustRootStoreError::Invalid(_))
        ));
    }
}
