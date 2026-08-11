use super::plugin_catalog_support::{find_registry, map_catalog_error};
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::IPluginRegistryCatalog;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use a3s_use_extension::{PluginCatalogHost, PluginCatalogPage, PluginCatalogSearch};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SearchPluginCatalog {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
    pub host: PluginCatalogHost,
    pub search: PluginCatalogSearch,
}

impl Query for SearchPluginCatalog {
    type Output = ApplicationResult<PluginCatalogPage>;
}

pub struct SearchPluginCatalogHandler {
    registries: Arc<dyn IPluginRegistryRepository>,
    catalog: Arc<dyn IPluginRegistryCatalog>,
}

impl SearchPluginCatalogHandler {
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

impl QueryHandler<SearchPluginCatalog> for SearchPluginCatalogHandler {
    fn execute(
        &self,
        query: SearchPluginCatalog,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginCatalogPage>>> {
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
                .search(&registry, &query.host, &query.search)
                .await
                .map_err(map_catalog_error))
        })
    }
}

#[derive(Debug, Clone)]
pub struct SearchCachedPluginCatalog {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
    pub host: PluginCatalogHost,
    pub search: PluginCatalogSearch,
}

impl Query for SearchCachedPluginCatalog {
    type Output = ApplicationResult<PluginCatalogPage>;
}

pub type SearchCachedPluginCatalogHandler = SearchPluginCatalogHandler;

impl QueryHandler<SearchCachedPluginCatalog> for SearchCachedPluginCatalogHandler {
    fn execute(
        &self,
        query: SearchCachedPluginCatalog,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginCatalogPage>>> {
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
                .search_cached(&registry, &query.host, &query.search)
                .await
                .map_err(map_catalog_error))
        })
    }
}
