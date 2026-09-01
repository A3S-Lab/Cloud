mod api_contract;
mod api_response_interceptor;
mod management_mcp;
mod oauth_transport;
mod polling_sse;
mod request_context;
mod request_id_middleware;
mod sequence_stream;

use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationAdministratorGuard;
pub(crate) use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use a3s_boot::{BootError, BootRequest, ControllerDefinition, Result, AUTH_SCOPES_METADATA};

/// Applies the single root-owned HTTP policy for an organization administrator
/// read. Product controllers do not import Identity presentation guards or
/// reproduce their credential-scope metadata.
pub(crate) fn organization_administrator_read_controller(
    controller: ControllerDefinition,
) -> Result<ControllerDefinition> {
    controller
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])
}

fn organization_tenant_scoped_controller(
    controller: ControllerDefinition,
    required_scope: &'static str,
) -> Result<ControllerDefinition> {
    controller
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![required_scope])
}

pub(crate) fn organization_tenant_file_write_controller(
    controller: ControllerDefinition,
) -> Result<ControllerDefinition> {
    organization_tenant_scoped_controller(controller, ApiTokenScope::FILE_WRITE)
}

pub(crate) fn organization_tenant_asset_write_controller(
    controller: ControllerDefinition,
) -> Result<ControllerDefinition> {
    organization_tenant_scoped_controller(controller, ApiTokenScope::ASSET_WRITE)
}

pub(crate) fn organization_tenant_cloud_read_controller(
    controller: ControllerDefinition,
) -> Result<ControllerDefinition> {
    organization_tenant_scoped_controller(controller, ApiTokenScope::CLOUD_READ)
}

fn require_request_scope(request: &BootRequest, scope: &str) -> Result<()> {
    if request.require_auth_principal()?.has_scope(scope) {
        return Ok(());
    }
    Err(BootError::Forbidden(
        "authenticated token does not have the required scope".into(),
    ))
}

pub(crate) fn require_asset_write_scope(request: &BootRequest) -> Result<()> {
    require_request_scope(request, ApiTokenScope::ASSET_WRITE)
}

pub(crate) fn require_cloud_read_scope(request: &BootRequest) -> Result<()> {
    require_request_scope(request, ApiTokenScope::CLOUD_READ)
}

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
pub(crate) use request_context::{
    actor_principal_id, asset_access, request_id, request_identity, search_visibility,
    user_file_access,
};
pub use request_id_middleware::RequestIdMiddleware;
pub(crate) use sequence_stream::{
    decode_sequence_cursor, default_live_sequence_limit, format_sequence_cursor,
    parse_sequence_cursor, resolve_sequence_cursor, sequence_stream_error, stream_sequence_pages,
    SequencePage, SequenceRecord, MAX_LIVE_SEQUENCE_RECORDS,
};
