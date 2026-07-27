mod api_contract;
mod api_response_interceptor;
mod management_mcp;
mod request_id_middleware;

pub use api_contract::{
    generate_openapi_contract, openapi_info, ApiContractModule, API_CONTRACT_VERSION_HEADER,
    API_MAJOR_VERSION, API_PREFIX, MINIMUM_DEPRECATION_DAYS, OPENAPI_CONTRACT_VERSION,
    OPENAPI_DOCUMENT_PATH, OPENAPI_PUBLIC_PATH,
};
pub(crate) use api_response_interceptor::{
    api_success_envelope, application_error_envelope, boot_error_response,
};
pub use api_response_interceptor::{
    application_error_response, ApiErrorFilter, ApiResponseInterceptor,
};
pub use management_mcp::{ManagementMcpModule, MANAGEMENT_MCP_PROTOCOL_VERSION};
pub use request_id_middleware::RequestIdMiddleware;
