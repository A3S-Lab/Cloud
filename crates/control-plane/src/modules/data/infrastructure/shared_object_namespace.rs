use crate::infrastructure::{
    ConditionalObjectError, ConditionalObjectRead, ConditionalObjectVersion, ImmutableObjectClient,
    ImmutableObjectError,
};
use crate::modules::data::domain::{
    IObjectNamespace, ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceRead,
    ObjectNamespaceVersion,
};
use async_trait::async_trait;

#[async_trait]
impl IObjectNamespace for ImmutableObjectClient {
    async fn conditional_create(
        &self,
        object_key: &ObjectNamespaceKey,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError> {
        let write = ImmutableObjectClient::conditional_create(
            self,
            object_key.as_str(),
            body,
            maximum_bytes,
        )
        .await
        .map_err(map_conditional_error)?;
        map_version(write.version)
    }

    async fn conditional_overwrite(
        &self,
        object_key: &ObjectNamespaceKey,
        expected: &ObjectNamespaceVersion,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError> {
        let expected = ConditionalObjectVersion::from_parts(
            expected.e_tag().map(str::to_owned),
            expected.version().map(str::to_owned),
        )
        .map_err(map_conditional_error)?;
        let write = ImmutableObjectClient::conditional_overwrite(
            self,
            object_key.as_str(),
            &expected,
            body,
            maximum_bytes,
        )
        .await
        .map_err(map_conditional_error)?;
        map_version(write.version)
    }

    async fn read(
        &self,
        object_key: &ObjectNamespaceKey,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceRead, ObjectNamespaceError> {
        match ImmutableObjectClient::conditional_get(self, object_key.as_str(), maximum_bytes)
            .await
            .map_err(map_conditional_error)?
        {
            ConditionalObjectRead::Found { body, version } => Ok(ObjectNamespaceRead::Found {
                body,
                version: map_version(version)?,
            }),
            ConditionalObjectRead::Missing => Ok(ObjectNamespaceRead::Missing),
            ConditionalObjectRead::Corrupt => Ok(ObjectNamespaceRead::Corrupt),
        }
    }

    async fn delete(&self, object_key: &ObjectNamespaceKey) -> Result<(), ObjectNamespaceError> {
        self.remove(object_key.as_str())
            .await
            .map_err(map_immutable_error)
    }
}

fn map_version(
    version: ConditionalObjectVersion,
) -> Result<ObjectNamespaceVersion, ObjectNamespaceError> {
    ObjectNamespaceVersion::new(
        version.e_tag().map(str::to_owned),
        version.version().map(str::to_owned),
    )
    .map_err(ObjectNamespaceError::Corrupt)
}

fn map_conditional_error(error: ConditionalObjectError) -> ObjectNamespaceError {
    match error {
        ConditionalObjectError::Invalid(message) => ObjectNamespaceError::Invalid(message),
        ConditionalObjectError::Precondition(message) => {
            ObjectNamespaceError::Precondition(message)
        }
        ConditionalObjectError::Corrupt(message) => ObjectNamespaceError::Corrupt(message),
        ConditionalObjectError::Unsupported(message) => ObjectNamespaceError::Unsupported(message),
        ConditionalObjectError::Unavailable(message) => ObjectNamespaceError::Unavailable(message),
    }
}

fn map_immutable_error(error: ImmutableObjectError) -> ObjectNamespaceError {
    match error {
        ImmutableObjectError::Invalid(message) => ObjectNamespaceError::Invalid(message),
        ImmutableObjectError::Conflict(message) => ObjectNamespaceError::Precondition(message),
        ImmutableObjectError::Integrity(message) => ObjectNamespaceError::Corrupt(message),
        ImmutableObjectError::Unavailable(message) => ObjectNamespaceError::Unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::DisposableS3TestContext;
    use crate::modules::data::ObjectNamespaceConformanceProbe;
    use crate::modules::shared_kernel::domain::StorageNamespaceId;
    use object_store::memory::InMemory;
    use object_store::ObjectStore;
    use std::sync::Arc;

    #[tokio::test]
    async fn shared_remote_client_passes_destructive_cas_probe_and_cleans_up() {
        let client = ImmutableObjectClient::from_store(
            Arc::new(InMemory::new()) as Arc<dyn ObjectStore>,
            "durable-cells/test-namespace",
        )
        .expect("shared object client");
        let namespace: Arc<dyn IObjectNamespace> = Arc::new(client.clone());
        let probe = ObjectNamespaceConformanceProbe::new(namespace, 1024).expect("probe");
        probe
            .run(StorageNamespaceId::new())
            .await
            .expect("CAS conformance");

        let objects = client
            .conditional_get(".a3s-conformance/cas/missing", 1024)
            .await
            .expect("post-probe read");
        assert_eq!(objects, ConditionalObjectRead::Missing);
    }

    #[tokio::test]
    async fn uncertified_local_backend_fails_closed_for_cas() {
        let directory = tempfile::tempdir().expect("object directory");
        let client = ImmutableObjectClient::local(directory.path(), "durable-cells/local")
            .expect("local client");
        assert!(matches!(
            IObjectNamespace::conditional_create(
                &client,
                &ObjectNamespaceKey::parse("owner").expect("key"),
                b"value".to_vec(),
                32
            )
            .await,
            Err(ObjectNamespaceError::Unsupported(_))
        ));
    }

    #[tokio::test]
    #[ignore = "requires an explicitly configured disposable S3-compatible bucket"]
    async fn real_s3_compatible_namespace_passes_destructive_cas_conformance() {
        let context = DisposableS3TestContext::from_environment("s0-cas")
            .expect("disposable S3 test context");
        assert!(
            context.uses_secure_transport(),
            "S0 provider certification requires an HTTPS S3-compatible endpoint"
        );
        let namespace: Arc<dyn IObjectNamespace> = Arc::new(context.client());
        let evidence = ObjectNamespaceConformanceProbe::new(namespace, 1024)
            .expect("probe")
            .run(StorageNamespaceId::new())
            .await
            .expect("real S3 CAS conformance");
        evidence.validate().expect("complete conformance evidence");
        println!(
            "A3S_CLOUD_S0_NAMESPACE_PROVIDER_CERTIFIED provider=s3-compatible protocol=a3s.s0.object-namespace.v1 checks=7/7 cleanup=verified"
        );
    }

    #[test]
    fn s0_adapter_reuses_the_shared_object_client_only() {
        let source = include_str!("shared_object_namespace.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "object_store::",
            "std::fs::",
            "tokio::fs::",
            "AmazonS3Builder",
            "PutMode::",
            "reqwest::",
        ] {
            assert!(
                !production.contains(forbidden),
                "the S0 adapter must reuse ImmutableObjectClient; found {forbidden}"
            );
        }
    }
}
