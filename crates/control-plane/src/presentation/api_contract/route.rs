use super::{
    API_CONTRACT_VERSION_HEADER, API_PREFIX, OPENAPI_CONTRACT_VERSION, OPENAPI_DOCUMENT_PATH,
};
use a3s_boot::{BootResponse, Module, OpenApiInfo, Result, RouteDefinition, AUTH_PUBLIC_METADATA};

const OPENAPI_DOCUMENT: &str = include_str!("../../../../../openapi/v1.json");

#[derive(Debug, Clone, Copy, Default)]
pub struct ApiContractModule;

impl Module for ApiContractModule {
    fn name(&self) -> &'static str {
        "api-contract"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            OPENAPI_DOCUMENT_PATH,
            |_| async {
                Ok(BootResponse::new(200, OPENAPI_DOCUMENT.as_bytes())
                    .with_header("content-type", "application/json")
                    .with_header("cache-control", "public, max-age=300")
                    .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION)
                    .with_header("x-a3s-api-envelope", "1"))
            },
        )?
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .hide_from_openapi()])
    }
}

pub fn openapi_info() -> OpenApiInfo {
    OpenApiInfo::new("A3S Cloud REST API", OPENAPI_CONTRACT_VERSION)
        .with_description(
            "Stable version 1 REST contract shared by the A3S Cloud web console and CLI.",
        )
        .with_server_description(API_PREFIX, "A3S Cloud REST API v1")
}
