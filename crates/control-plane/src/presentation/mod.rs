mod api_contract;
mod api_response_interceptor;
mod management_mcp;
mod oauth_transport;
mod polling_sse;
mod request_context;
mod request_id_middleware;
mod sequence_stream;

pub(crate) use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};

pub(crate) const A3S_ACL_MEDIA_TYPE: &str = "application/vnd.a3s.acl";

pub(crate) fn bounded_acl_document(
    request: &a3s_boot::BootRequest,
    maximum_bytes: usize,
    label: &str,
) -> a3s_boot::Result<String> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|value| value.eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE)) {
        return Err(a3s_boot::BootError::UnsupportedMediaType(format!(
            "{label} requires {A3S_ACL_MEDIA_TYPE}"
        )));
    }
    if request.body().is_empty() {
        return Err(a3s_boot::BootError::BadRequest(format!(
            "{label} ACL body is required"
        )));
    }
    if request.body().len() > maximum_bytes {
        return Err(a3s_boot::BootError::PayloadTooLarge(format!(
            "{label} ACL exceeds {maximum_bytes} bytes"
        )));
    }
    std::str::from_utf8(request.body())
        .map(str::to_owned)
        .map_err(|_| a3s_boot::BootError::BadRequest(format!("{label} ACL must be valid UTF-8")))
}

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
pub(crate) use oauth_transport::{
    bounded_oauth_query_pairs, oauth_callback_query, oauth_no_store, OAuthNoStoreErrorFilter,
};
pub(crate) use polling_sse::{polling_sse_stream, PollingSseInitial, PollingSseOptions};
pub(crate) use request_context::{actor_principal_id, request_id, request_identity};
pub use request_id_middleware::RequestIdMiddleware;
pub(crate) use sequence_stream::{
    decode_sequence_cursor, default_live_sequence_limit, format_sequence_cursor,
    parse_sequence_cursor, resolve_sequence_cursor, sequence_stream_error, stream_sequence_pages,
    SequencePage, SequenceRecord, MAX_LIVE_SEQUENCE_RECORDS,
};
