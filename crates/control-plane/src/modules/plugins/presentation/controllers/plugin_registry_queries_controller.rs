use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::plugins::application::{
    GetPluginRegistry, InspectCachedPluginCatalog, InspectPluginCatalog, ListPluginRegistries,
    SearchCachedPluginCatalog, SearchPluginCatalog,
};
use crate::modules::plugins::presentation::dto::{
    PluginCatalogInspectRequest, PluginCatalogSearchRequest, PluginRegistryResponse,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_registry_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(PluginRegistryQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct PluginRegistryQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl PluginRegistryQueriesController {
    #[get("/{organization_id}/plugin-registries", raw)]
    async fn list_registries(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id = organization_id(&request)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get("/{organization_id}/plugin-registries/{registry_id}", raw)]
    async fn get_registry(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[post("/{organization_id}/plugin-registries/{registry_id}/catalog/search", raw)]
    async fn search_catalog(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PluginCatalogSearchRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[post(
        "/{organization_id}/plugin-registries/{registry_id}/catalog/cache/search",
        raw
    )]
    async fn search_cached_catalog(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PluginCatalogSearchRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[post("/{organization_id}/plugin-registries/{registry_id}/catalog/inspect", raw)]
    async fn inspect_catalog(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PluginCatalogInspectRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[post(
        "/{organization_id}/plugin-registries/{registry_id}/catalog/cache/inspect",
        raw
    )]
    async fn inspect_cached_catalog(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PluginCatalogInspectRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

#[cfg(test)]
mod nest_macro_plugin_registry_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn plugin_registry_queries_register_scoped_guarded_routes_via_nest_macros() {
        let controller = plugin_registry_queries_controller(Arc::new(QueryBus::new()))
            .expect("plugin registry nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 6);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/plugin-registries".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/plugin-registries/{registry_id}".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/search"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/cache/search"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/inspect"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/cache/inspect"
                .into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::CLOUD_READ])
            );
        }
    }
}
