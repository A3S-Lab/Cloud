use crate::infrastructure::{ImmutableObjectClient, ImmutableObjectError, ImmutableObjectRead};
use crate::modules::plugins::domain::services::{
    IPluginPolicyStore, PluginPolicyStoreError, PluginPolicyWrite,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use std::path::PathBuf;

const OBJECT_NAMESPACE: &str = "plugin-policies";

#[derive(Debug, Clone)]
pub struct PluginPolicyObjectStore {
    objects: ImmutableObjectClient,
    maximum_policy_bytes: u64,
}

impl PluginPolicyObjectStore {
    pub fn local(
        root: impl Into<PathBuf>,
        maximum_policy_bytes: u64,
    ) -> Result<Self, PluginPolicyStoreError> {
        let objects =
            ImmutableObjectClient::local(root, OBJECT_NAMESPACE).map_err(map_object_error)?;
        Self::from_client(objects, maximum_policy_bytes)
    }

    pub(crate) fn from_client(
        objects: ImmutableObjectClient,
        maximum_policy_bytes: u64,
    ) -> Result<Self, PluginPolicyStoreError> {
        if maximum_policy_bytes == 0 {
            return Err(PluginPolicyStoreError::Invalid(
                "plugin policy size bound must be positive".into(),
            ));
        }
        Ok(Self {
            objects,
            maximum_policy_bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory(maximum_policy_bytes: u64) -> Result<Self, PluginPolicyStoreError> {
        let objects: std::sync::Arc<dyn object_store::ObjectStore> =
            std::sync::Arc::new(object_store::memory::InMemory::new());
        let client = ImmutableObjectClient::from_store(objects, OBJECT_NAMESPACE)
            .map_err(map_object_error)?;
        Self::from_client(client, maximum_policy_bytes)
    }

    fn validate_bytes(
        &self,
        digest: &Sha256Digest,
        bytes: &[u8],
    ) -> Result<(), PluginPolicyStoreError> {
        if bytes.is_empty() || bytes.len() as u64 > self.maximum_policy_bytes {
            return Err(PluginPolicyStoreError::Invalid(
                "plugin policy bytes are empty or exceed the configured bound".into(),
            ));
        }
        let actual = Sha256Digest::from_bytes(bytes);
        if &actual != digest {
            return Err(PluginPolicyStoreError::Integrity(
                "plugin policy bytes do not match their content address".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IPluginPolicyStore for PluginPolicyObjectStore {
    async fn put(
        &self,
        digest: &Sha256Digest,
        bytes: Vec<u8>,
    ) -> Result<PluginPolicyWrite, PluginPolicyStoreError> {
        self.validate_bytes(digest, &bytes)?;
        let write = self
            .objects
            .put(digest.as_str(), bytes, self.maximum_policy_bytes)
            .await
            .map_err(map_object_error)?;
        Ok(PluginPolicyWrite {
            replayed: !write.created,
        })
    }

    async fn get(&self, digest: &Sha256Digest) -> Result<Vec<u8>, PluginPolicyStoreError> {
        let bytes = match self
            .objects
            .get(digest.as_str(), self.maximum_policy_bytes)
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectRead::Found(bytes) => bytes,
            ImmutableObjectRead::Missing => return Err(PluginPolicyStoreError::NotFound),
            ImmutableObjectRead::Corrupt => {
                return Err(PluginPolicyStoreError::Integrity(
                    "stored plugin policy exceeds its admission bound".into(),
                ))
            }
        };
        self.validate_bytes(digest, &bytes)?;
        Ok(bytes)
    }
}

fn map_object_error(error: ImmutableObjectError) -> PluginPolicyStoreError {
    match error {
        ImmutableObjectError::Invalid(message) => PluginPolicyStoreError::Invalid(message),
        ImmutableObjectError::Conflict(_) => PluginPolicyStoreError::Conflict,
        ImmutableObjectError::Integrity(message) => PluginPolicyStoreError::Integrity(message),
        ImmutableObjectError::Unsupported(message) => PluginPolicyStoreError::Storage(message),
        ImmutableObjectError::Unavailable(message) => PluginPolicyStoreError::Storage(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get_round_trip_by_digest() {
        let store = PluginPolicyObjectStore::in_memory(1024).expect("store");
        let bytes = b"default = \"deny\"\n".to_vec();
        let digest = Sha256Digest::from_bytes(&bytes);
        let write = store.put(&digest, bytes.clone()).await.expect("put");
        assert!(!write.replayed);
        assert_eq!(store.get(&digest).await.expect("get"), bytes);
        let replay = store.put(&digest, bytes).await.expect("replay");
        assert!(replay.replayed);
    }
}
