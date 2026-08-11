use super::plugin_catalog_support::{find_registry, map_catalog_error};
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::IPluginRegistryCatalog;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{PluginCatalogHost, PluginCatalogInspection};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct InspectPluginCatalog {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
    pub host: PluginCatalogHost,
    pub package_id: String,
    pub version: Option<String>,
    pub channel: Option<PluginReleaseChannel>,
}

impl Query for InspectPluginCatalog {
    type Output = ApplicationResult<PluginCatalogInspection>;
}

pub struct InspectPluginCatalogHandler {
    registries: Arc<dyn IPluginRegistryRepository>,
    catalog: Arc<dyn IPluginRegistryCatalog>,
}

impl InspectPluginCatalogHandler {
    pub fn new(
        registries: Arc<dyn IPluginRegistryRepository>,
        catalog: Arc<dyn IPluginRegistryCatalog>,
    ) -> Self {
        Self {
            registries,
            catalog,
        }
    }
}

impl QueryHandler<InspectPluginCatalog> for InspectPluginCatalogHandler {
    fn execute(
        &self,
        query: InspectPluginCatalog,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginCatalogInspection>>>
    {
        let registries = Arc::clone(&self.registries);
        let catalog = Arc::clone(&self.catalog);
        Box::pin(async move {
            let registry = find_registry(
                registries.as_ref(),
                query.organization_id,
                query.registry_id,
            )
            .await;
            let registry = match registry {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            Ok(catalog
                .inspect(
                    &registry,
                    &query.host,
                    &query.package_id,
                    query.version.as_deref(),
                    query.channel,
                )
                .await
                .map_err(map_catalog_error))
        })
    }
}

#[derive(Debug, Clone)]
pub struct InspectCachedPluginCatalog {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
    pub host: PluginCatalogHost,
    pub package_id: String,
    pub version: Option<String>,
    pub channel: Option<PluginReleaseChannel>,
}

impl Query for InspectCachedPluginCatalog {
    type Output = ApplicationResult<PluginCatalogInspection>;
}

pub type InspectCachedPluginCatalogHandler = InspectPluginCatalogHandler;

impl QueryHandler<InspectCachedPluginCatalog> for InspectCachedPluginCatalogHandler {
    fn execute(
        &self,
        query: InspectCachedPluginCatalog,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginCatalogInspection>>>
    {
        let registries = Arc::clone(&self.registries);
        let catalog = Arc::clone(&self.catalog);
        Box::pin(async move {
            let registry = find_registry(
                registries.as_ref(),
                query.organization_id,
                query.registry_id,
            )
            .await;
            let registry = match registry {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            Ok(catalog
                .inspect_cached(
                    &registry,
                    &query.host,
                    &query.package_id,
                    query.version.as_deref(),
                    query.channel,
                )
                .await
                .map_err(map_catalog_error))
        })
    }
}
