use crate::NodeControlClientError;
use a3s_cloud_contracts::CloudSecretReference;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use zeroize::Zeroize;

#[cfg(target_os = "linux")]
use a3s_box_runtime::{
    BoxRegistryCredential, BoxSecretMaterial, BoxSecretMaterializationError, BoxSecretMaterializer,
};
#[cfg(target_os = "linux")]
use a3s_cloud_contracts::RegistryCredentialMaterial;
#[cfg(target_os = "linux")]
use a3s_runtime::contract::SecretReference;
use a3s_runtime::{RuntimeError, RuntimeResult};

#[async_trait]
pub trait NodeSecretTransport: Send + Sync {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError>;
}

/// The sole adapter from Cloud's authenticated node Secret channel to A3S
/// Box's provider-neutral materialization port.
///
/// The adapter is installed in the Box driver before enrollment so the
/// advertised Runtime capabilities are stable. The existing reloadable node
/// transport is bound exactly once after enrollment and remains the authority
/// across certificate rotations.
pub(crate) struct CloudBoxSecretMaterializer {
    transport: RwLock<Option<Arc<dyn NodeSecretTransport>>>,
}

impl CloudBoxSecretMaterializer {
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn new() -> Self {
        Self {
            transport: RwLock::new(None),
        }
    }

    pub(crate) async fn bind_transport(
        &self,
        transport: Arc<dyn NodeSecretTransport>,
    ) -> RuntimeResult<()> {
        let mut current = self.transport.write().await;
        match current.as_ref() {
            Some(existing) if Arc::ptr_eq(existing, &transport) => Ok(()),
            Some(_) => Err(RuntimeError::RequestConflict {
                request_id: "box-secret-transport-binding".into(),
            }),
            None => {
                *current = Some(transport);
                Ok(())
            }
        }
    }

    #[cfg(target_os = "linux")]
    async fn transport(
        &self,
    ) -> Result<Arc<dyn NodeSecretTransport>, BoxSecretMaterializationError> {
        self.transport.read().await.clone().ok_or_else(|| {
            BoxSecretMaterializationError::Unavailable(
                "Cloud node Secret transport is not bound".into(),
            )
        })
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl BoxSecretMaterializer for CloudBoxSecretMaterializer {
    async fn materialize(
        &self,
        reference: &SecretReference,
    ) -> Result<BoxSecretMaterial, BoxSecretMaterializationError> {
        let reference = parse_cloud_reference(&reference.reference)?;
        let material = self
            .transport()
            .await?
            .resolve_secret(reference)
            .await
            .map_err(secret_transport_error)?;
        BoxSecretMaterial::new(material.as_bytes().to_vec())
    }

    async fn materialize_registry_credential(
        &self,
        reference: &SecretReference,
        _registry: &str,
    ) -> Result<BoxRegistryCredential, BoxSecretMaterializationError> {
        let reference = parse_cloud_reference(&reference.reference)?;
        let material = self
            .transport()
            .await?
            .resolve_secret(reference)
            .await
            .map_err(secret_transport_error)?;
        let credential = RegistryCredentialMaterial::parse(material.as_bytes()).map_err(|_| {
            BoxSecretMaterializationError::Rejected(
                "Cloud registry credential Secret material is invalid".into(),
            )
        })?;
        BoxRegistryCredential::new(
            credential.username().to_owned(),
            credential.password().to_owned(),
        )
    }
}

#[cfg(target_os = "linux")]
fn parse_cloud_reference(
    reference: &str,
) -> Result<CloudSecretReference, BoxSecretMaterializationError> {
    CloudSecretReference::parse(reference).map_err(|_| {
        BoxSecretMaterializationError::Rejected("Cloud Secret reference is invalid".into())
    })
}

#[cfg(target_os = "linux")]
fn secret_transport_error(error: NodeControlClientError) -> BoxSecretMaterializationError {
    if error.retryable() {
        BoxSecretMaterializationError::Unavailable(
            "Cloud Secret material is temporarily unavailable".into(),
        )
    } else {
        BoxSecretMaterializationError::Rejected("Cloud Secret material request was rejected".into())
    }
}

#[derive(PartialEq, Eq)]
pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    pub fn new(value: impl Into<Vec<u8>>) -> Result<Self, String> {
        let mut value = value.into();
        if value.is_empty() || value.len() > 1024 * 1024 {
            value.zeroize();
            return Err("Secret material must contain between 1 byte and 1 MiB".into());
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted-secret-material>")
    }
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FixedSecretTransport {
        expected: CloudSecretReference,
        value: Vec<u8>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl NodeSecretTransport for FixedSecretTransport {
        async fn resolve_secret(
            &self,
            reference: CloudSecretReference,
        ) -> Result<SecretMaterial, NodeControlClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if reference != self.expected {
                return Err(NodeControlClientError::Invalid(
                    "unexpected fixture reference".into(),
                ));
            }
            SecretMaterial::new(self.value.clone()).map_err(NodeControlClientError::Invalid)
        }
    }

    #[test]
    fn material_debug_output_is_redacted() {
        let material = SecretMaterial::new(b"never-log-this".to_vec()).expect("Secret material");
        assert_eq!(format!("{material:?}"), "<redacted-secret-material>");
    }

    #[tokio::test]
    async fn box_materializer_fences_one_reloadable_node_transport() {
        let reference = CloudSecretReference::new(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), 3)
            .expect("Cloud Secret reference");
        let first = Arc::new(FixedSecretTransport {
            expected: reference,
            value: b"first".to_vec(),
            calls: AtomicUsize::new(0),
        });
        let second = Arc::new(FixedSecretTransport {
            expected: reference,
            value: b"second".to_vec(),
            calls: AtomicUsize::new(0),
        });
        let materializer = CloudBoxSecretMaterializer::new();
        let binding: Arc<dyn NodeSecretTransport> = first.clone();

        materializer
            .bind_transport(binding.clone())
            .await
            .expect("first transport binding");
        materializer
            .bind_transport(binding)
            .await
            .expect("idempotent transport binding");
        let conflicting: Arc<dyn NodeSecretTransport> = second;
        assert!(matches!(
            materializer.bind_transport(conflicting).await,
            Err(RuntimeError::RequestConflict { .. })
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn box_materializer_resolves_cloud_bytes_and_typed_registry_credentials() {
        let reference = CloudSecretReference::new(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), 5)
            .expect("Cloud Secret reference");
        let ordinary_transport = Arc::new(FixedSecretTransport {
            expected: reference,
            value: b"box-secret-value".to_vec(),
            calls: AtomicUsize::new(0),
        });
        let materializer = CloudBoxSecretMaterializer::new();
        let binding: Arc<dyn NodeSecretTransport> = ordinary_transport.clone();
        materializer
            .bind_transport(binding)
            .await
            .expect("Secret transport binding");
        let runtime_reference = SecretReference {
            name: "provider-token".into(),
            reference: reference.to_string(),
            target: a3s_runtime::contract::SecretTarget::Environment {
                variable: "PROVIDER_TOKEN".into(),
            },
        };

        let material = materializer
            .materialize(&runtime_reference)
            .await
            .expect("Box Secret material");
        assert_eq!(material.as_bytes(), b"box-secret-value");
        assert_eq!(ordinary_transport.calls.load(Ordering::SeqCst), 1);

        let registry_transport = Arc::new(FixedSecretTransport {
            expected: reference,
            value: br#"{"schema":"a3s.cloud.registry-credential.v1","username":"registry-user","password":"registry-password"}"#
                .to_vec(),
            calls: AtomicUsize::new(0),
        });
        let registry_materializer = CloudBoxSecretMaterializer::new();
        let binding: Arc<dyn NodeSecretTransport> = registry_transport.clone();
        registry_materializer
            .bind_transport(binding)
            .await
            .expect("registry transport binding");
        let credential = registry_materializer
            .materialize_registry_credential(&runtime_reference, "registry.example")
            .await
            .expect("Box registry credential");
        assert_eq!(credential.username(), "registry-user");
        assert_eq!(credential.password(), "registry-password");
        assert_eq!(registry_transport.calls.load(Ordering::SeqCst), 1);
    }
}
