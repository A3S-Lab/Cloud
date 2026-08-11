use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginTrustRootStore, PluginRegistryCatalogError,
};
use crate::modules::plugins::domain::value_objects::PluginRegistryState;
use a3s_use_core::{PluginReleaseChannel, UseError};
use a3s_use_extension::{
    inspect_cached_plugin, inspect_remote_plugin, refresh_remote_registry, search_cached_plugins,
    search_remote_plugins, PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage,
    PluginCatalogSearch, RegistryNetworkPolicy, TrustedRegistry, VerifiedRegistryMetadata,
};
use async_trait::async_trait;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct A3sUsePluginRegistryCatalog {
    trust_roots: Arc<dyn IPluginTrustRootStore>,
    metadata_root: PathBuf,
}

impl A3sUsePluginRegistryCatalog {
    pub fn new(
        trust_roots: Arc<dyn IPluginTrustRootStore>,
        metadata_root: impl Into<PathBuf>,
    ) -> Result<Self, PluginRegistryCatalogError> {
        let metadata_root = metadata_root.into();
        if !safe_absolute_root(&metadata_root) {
            return Err(PluginRegistryCatalogError::Invalid(
                "A3S Use metadata root must be an absolute, non-root normalized path".into(),
            ));
        }
        Ok(Self {
            trust_roots,
            metadata_root,
        })
    }

    async fn trusted_registry(
        &self,
        registry: &PluginRegistry,
    ) -> Result<TrustedRegistry, PluginRegistryCatalogError> {
        registry
            .validate()
            .map_err(PluginRegistryCatalogError::Invalid)?;
        if registry.state != PluginRegistryState::Active {
            return Err(PluginRegistryCatalogError::Disabled);
        }

        let root_bytes = self.trust_roots.get(&registry.trust_root).await?;
        let root_digest = registry
            .trust_root
            .digest()
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| {
                PluginRegistryCatalogError::Invalid(
                    "plugin trust-root digest is not canonical SHA-256".into(),
                )
            })?;
        let datastore = self
            .metadata_root
            .join("organizations")
            .join(registry.organization_id.to_string())
            .join("registries")
            .join(registry.id.to_string())
            .join(root_digest);
        let trusted = TrustedRegistry::new(
            format!("cloud-{}", registry.id),
            registry.endpoint.as_str(),
            root_digest,
            None,
            datastore,
        )
        .map_err(map_use_error)?
        .with_network_policy(RegistryNetworkPolicy::PublicInternet);
        let evidence = trusted
            .pin_trusted_root(&root_bytes)
            .await
            .map_err(map_use_error)?;
        let size_bytes = u64::try_from(root_bytes.len()).map_err(|_| {
            PluginRegistryCatalogError::Invalid(
                "plugin trust-root byte length exceeds the supported range".into(),
            )
        })?;
        if evidence.root_sha256 != root_digest
            || evidence.root_version != registry.trust_root.version()
            || evidence.size_bytes != size_bytes
        {
            return Err(PluginRegistryCatalogError::TrustRootEvidenceMismatch);
        }
        Ok(trusted)
    }
}

#[async_trait]
impl IPluginRegistryCatalog for A3sUsePluginRegistryCatalog {
    async fn refresh(
        &self,
        registry: &PluginRegistry,
    ) -> Result<VerifiedRegistryMetadata, PluginRegistryCatalogError> {
        let trusted = self.trusted_registry(registry).await?;
        refresh_remote_registry(&trusted)
            .await
            .map_err(map_use_error)
    }

    async fn search(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        let trusted = self.trusted_registry(registry).await?;
        search_remote_plugins(&trusted, host, search)
            .await
            .map_err(map_use_error)
    }

    async fn search_cached(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        let trusted = self.trusted_registry(registry).await?;
        search_cached_plugins(&trusted, host, search)
            .await
            .map_err(map_use_error)
    }

    async fn inspect(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        package_id: &str,
        version: Option<&str>,
        channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        let trusted = self.trusted_registry(registry).await?;
        inspect_remote_plugin(&trusted, host, package_id, version, channel)
            .await
            .map_err(map_use_error)
    }

    async fn inspect_cached(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        package_id: &str,
        version: Option<&str>,
        channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        let trusted = self.trusted_registry(registry).await?;
        inspect_cached_plugin(&trusted, host, package_id, version, channel)
            .await
            .map_err(map_use_error)
    }
}

fn safe_absolute_root(path: &Path) -> bool {
    path.is_absolute()
        && path.parent().is_some()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn map_use_error(error: UseError) -> PluginRegistryCatalogError {
    match error.code.as_str() {
        "use.extension.catalog_query_invalid" | "use.extension.catalog_cursor_invalid" => {
            PluginRegistryCatalogError::QueryInvalid
        }
        "use.extension.catalog_cursor_stale" => PluginRegistryCatalogError::CursorStale,
        "use.extension.catalog_package_missing" => PluginRegistryCatalogError::PackageNotFound,
        "use.extension.catalog_package_incompatible" => {
            PluginRegistryCatalogError::PackageIncompatible
        }
        _ => PluginRegistryCatalogError::Use { code: error.code },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::plugins::domain::services::IPluginTrustRootStore;
    use crate::modules::plugins::domain::value_objects::{PluginRegistryEndpoint, PluginTrustRoot};
    use crate::modules::plugins::test_support::VALID_BOOTSTRAP_ROOT;
    use crate::modules::plugins::PluginTrustRootObjectStore;
    use crate::modules::shared_kernel::domain::{
        OrganizationId, PluginRegistryId, PrincipalId, ResourceName, Sha256Digest,
    };
    use a3s_use_extension::MAX_BOOTSTRAP_ROOT_BYTES;
    use chrono::Utc;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use uuid::Uuid;

    fn trust_root(bytes: &[u8], version: u64) -> PluginTrustRoot {
        let digest = Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(bytes)))
            .expect("root digest");
        PluginTrustRoot::from_digest(digest, version).expect("trust root")
    }

    fn registry(root: PluginTrustRoot) -> PluginRegistry {
        PluginRegistry::enroll(
            OrganizationId::new(),
            PluginRegistryId::new(),
            ResourceName::parse("Official").expect("registry name"),
            PluginRegistryEndpoint::parse("https://registry.example/plugins")
                .expect("registry endpoint"),
            root,
            PrincipalId::new(),
            Uuid::now_v7(),
            Utc::now(),
        )
        .expect("plugin registry")
    }

    async fn catalog_adapter(
        temporary: &TempDir,
        bytes: &[u8],
        version: u64,
    ) -> (A3sUsePluginRegistryCatalog, PluginRegistry) {
        let root = trust_root(bytes, version);
        let store = Arc::new(
            PluginTrustRootObjectStore::in_memory(MAX_BOOTSTRAP_ROOT_BYTES)
                .expect("trust-root object store"),
        );
        store
            .put(&root, bytes.to_vec())
            .await
            .expect("stored trust root");
        let adapter =
            A3sUsePluginRegistryCatalog::new(store, temporary.path().join("use-registry-metadata"))
                .expect("catalog adapter");
        (adapter, registry(root))
    }

    #[tokio::test]
    async fn pins_exact_enrolled_root_under_tenant_scoped_public_use_registry() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let (adapter, registry) = catalog_adapter(&temporary, VALID_BOOTSTRAP_ROOT, 1).await;

        let trusted = adapter
            .trusted_registry(&registry)
            .await
            .expect("trusted registry");
        let replayed = adapter
            .trusted_registry(&registry)
            .await
            .expect("replayed trusted registry");

        assert_eq!(trusted.name(), format!("cloud-{}", registry.id));
        assert_eq!(trusted.base_url().as_str(), registry.endpoint.as_str());
        assert_eq!(
            trusted.network_policy(),
            RegistryNetworkPolicy::PublicInternet
        );
        assert_eq!(trusted.datastore(), replayed.datastore());
        assert!(trusted.datastore().starts_with(temporary.path()));
        assert!(trusted.datastore().join("bootstrap-root.json").is_file());
    }

    #[tokio::test]
    async fn rejects_wrong_bootstrap_version_and_malformed_tuf_through_use() {
        let wrong_version = tempfile::tempdir().expect("temporary directory");
        let (adapter, registry) = catalog_adapter(&wrong_version, VALID_BOOTSTRAP_ROOT, 2).await;
        assert!(matches!(
            adapter.trusted_registry(&registry).await,
            Err(PluginRegistryCatalogError::TrustRootEvidenceMismatch)
        ));

        let malformed = tempfile::tempdir().expect("temporary directory");
        let (adapter, registry) = catalog_adapter(&malformed, br#"{"signed":{}}"#, 1).await;
        assert!(matches!(
            adapter.trusted_registry(&registry).await,
            Err(PluginRegistryCatalogError::Use { code })
                if code == "use.extension.registry_root_invalid"
        ));
    }

    #[tokio::test]
    async fn rejects_disabled_registry_and_relative_metadata_authority() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let (adapter, mut registry) = catalog_adapter(&temporary, VALID_BOOTSTRAP_ROOT, 1).await;
        registry.state = PluginRegistryState::Disabled;
        assert!(matches!(
            adapter.trusted_registry(&registry).await,
            Err(PluginRegistryCatalogError::Disabled)
        ));

        let store = Arc::new(
            PluginTrustRootObjectStore::in_memory(MAX_BOOTSTRAP_ROOT_BYTES)
                .expect("trust-root object store"),
        );
        assert!(matches!(
            A3sUsePluginRegistryCatalog::new(store, "relative/metadata"),
            Err(PluginRegistryCatalogError::Invalid(_))
        ));
    }
}
