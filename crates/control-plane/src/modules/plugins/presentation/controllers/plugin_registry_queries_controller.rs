use crate::modules::plugins::application::{
    GetPluginRegistry, InspectCachedPluginCatalog, InspectPluginCatalog, ListPluginRegistries,
    SearchCachedPluginCatalog, SearchPluginCatalog,
};
use crate::modules::plugins::presentation::dto::{
    PluginCatalogInspectRequest, PluginCatalogSearchRequest, PluginRegistryResponse,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use crate::presentation::{
    application_error_response, organization_tenant_cloud_read_controller, request_id,
};
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_registry_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_bus = Arc::clone(&bus);
    let online_search_bus = Arc::clone(&bus);
    let cached_search_bus = Arc::clone(&bus);
    let online_inspect_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
        .get(
            "/{organization_id}/plugin-registries",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id = organization_id(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListPluginRegistries { organization_id })
                        .await?
                    {
                        Ok(registries) => BootResponse::json(
                            &registries
                                .into_iter()
                                .map(PluginRegistryResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/plugin-registries/{registry_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPluginRegistry {
                            organization_id: organization_id(&request)?,
                            registry_id: registry_id(&request)?,
                        })
                        .await?
                    {
                        Ok(registry) => BootResponse::json(&PluginRegistryResponse::from(registry)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/plugin-registries/{registry_id}/catalog/search",
            move |request: BootRequest| {
                let bus = Arc::clone(&online_search_bus);
                async move {
                    let body: PluginCatalogSearchRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(SearchPluginCatalog {
                            organization_id: organization_id(&request)?,
                            registry_id: registry_id(&request)?,
                            host: body.host,
                            search: body.search,
                        })
                        .await?
                    {
                        Ok(page) => BootResponse::json(&page),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/plugin-registries/{registry_id}/catalog/cache/search",
            move |request: BootRequest| {
                let bus = Arc::clone(&cached_search_bus);
                async move {
                    let body: PluginCatalogSearchRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(SearchCachedPluginCatalog {
                            organization_id: organization_id(&request)?,
                            registry_id: registry_id(&request)?,
                            host: body.host,
                            search: body.search,
                        })
                        .await?
                    {
                        Ok(page) => BootResponse::json(&page),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/plugin-registries/{registry_id}/catalog/inspect",
            move |request: BootRequest| {
                let bus = Arc::clone(&online_inspect_bus);
                async move {
                    let body: PluginCatalogInspectRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(InspectPluginCatalog {
                            organization_id: organization_id(&request)?,
                            registry_id: registry_id(&request)?,
                            host: body.host,
                            package_id: body.package_id,
                            version: body.version,
                            channel: body.channel,
                        })
                        .await?
                    {
                        Ok(inspection) => BootResponse::json(&inspection),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/plugin-registries/{registry_id}/catalog/cache/inspect",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: PluginCatalogInspectRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(InspectCachedPluginCatalog {
                            organization_id: organization_id(&request)?,
                            registry_id: registry_id(&request)?,
                            host: body.host,
                            package_id: body.package_id,
                            version: body.version,
                            channel: body.channel,
                        })
                        .await?
                    {
                        Ok(inspection) => BootResponse::json(&inspection),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?;
    organization_tenant_cloud_read_controller(controller)
}

fn organization_id(request: &BootRequest) -> Result<OrganizationId> {
    request
        .param_as::<Uuid>("organization_id")
        .map(OrganizationId::from_uuid)
}

fn registry_id(request: &BootRequest) -> Result<PluginRegistryId> {
    request
        .param_as::<Uuid>("registry_id")
        .map(PluginRegistryId::from_uuid)
}
