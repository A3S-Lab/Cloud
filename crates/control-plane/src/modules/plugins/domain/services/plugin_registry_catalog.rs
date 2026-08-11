use crate::modules::plugins::domain::entities::PluginRegistry;
use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{
    PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage, PluginCatalogSearch,
    VerifiedRegistryMetadata,
};
use async_trait::async_trait;

use super::PluginTrustRootStoreError;

#[derive(Debug, thiserror::Error)]
pub enum PluginRegistryCatalogError {
    #[error("plugin registry is invalid: {0}")]
    Invalid(String),
    #[error("plugin registry is disabled")]
    Disabled,
    #[error(transparent)]
    TrustRoot(#[from] PluginTrustRootStoreError),
    #[error("A3S Use Registry operation failed ({code})")]
    Use { code: String },
    #[error("pinned plugin trust-root evidence does not match the enrolled registry")]
    TrustRootEvidenceMismatch,
}

#[async_trait]
pub trait IPluginRegistryCatalog: Send + Sync {
    async fn refresh(
        &self,
        registry: &PluginRegistry,
    ) -> Result<VerifiedRegistryMetadata, PluginRegistryCatalogError>;

    async fn search(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError>;

    async fn search_cached(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError>;

    async fn inspect(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        package_id: &str,
        version: Option<&str>,
        channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError>;

    async fn inspect_cached(
        &self,
        registry: &PluginRegistry,
        host: &PluginCatalogHost,
        package_id: &str,
        version: Option<&str>,
        channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError>;
}
