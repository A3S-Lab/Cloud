use super::arguments::EmptyArguments;
use super::tool_result;
use crate::modules::plugins::{
    GetPluginRegistry, InspectCachedPluginCatalog, InspectPluginCatalog, ListPluginRegistries,
    PluginCatalogInspectRequest, PluginCatalogSearchRequest, PluginRegistryResponse,
    SearchCachedPluginCatalog, SearchPluginCatalog,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use a3s_boot::{QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginRegistryArguments {
    registry_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSearchArguments {
    registry_id: Uuid,
    #[serde(flatten)]
    request: PluginCatalogSearchRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogInspectArguments {
    registry_id: Uuid,
    #[serde(flatten)]
    request: PluginCatalogInspectRequest,
}

pub async fn list_registries(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListPluginRegistries { organization_id })
        .await?
    {
        Ok(registries) => tool_result::success(
            200,
            registries
                .into_iter()
                .map(PluginRegistryResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_registry(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginRegistryArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPluginRegistry {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
        })
        .await?
    {
        Ok(registry) => {
            tool_result::success(200, PluginRegistryResponse::from(registry), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn search_catalog(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginCatalogSearchArguments,
    request_id: Uuid,
    cached: bool,
) -> Result<Value> {
    let result = if cached {
        bus.execute(SearchCachedPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: arguments.request.host,
            search: arguments.request.search,
        })
        .await?
    } else {
        bus.execute(SearchPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: arguments.request.host,
            search: arguments.request.search,
        })
        .await?
    };
    match result {
        Ok(page) => tool_result::success(200, page, request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn inspect_catalog(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginCatalogInspectArguments,
    request_id: Uuid,
    cached: bool,
) -> Result<Value> {
    let request = arguments.request;
    let result = if cached {
        bus.execute(InspectCachedPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: request.host,
            package_id: request.package_id,
            version: request.version,
            channel: request.channel,
        })
        .await?
    } else {
        bus.execute(InspectPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: request.host,
            package_id: request.package_id,
            version: request.version,
            channel: request.channel,
        })
        .await?
    };
    match result {
        Ok(inspection) => tool_result::success(200, inspection, request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
