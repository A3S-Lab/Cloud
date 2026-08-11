use crate::modules::assets::domain::MCP_SERVICE_PROFILE_MAX_ACL_BYTES;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use crate::presentation::A3S_ACL_MEDIA_TYPE;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

pub(super) fn organization_id(request: &BootRequest) -> Result<OrganizationId> {
    Ok(OrganizationId::from_uuid(
        request.param_as::<Uuid>("organization_id")?,
    ))
}

pub(super) fn asset_ids(request: &BootRequest) -> Result<(OrganizationId, AssetId)> {
    Ok((
        organization_id(request)?,
        AssetId::from_uuid(request.param_as::<Uuid>("asset_id")?),
    ))
}

pub(super) fn asset_release_ids(
    request: &BootRequest,
) -> Result<(OrganizationId, AssetId, AssetReleaseId)> {
    let (organization_id, asset_id) = asset_ids(request)?;
    Ok((
        organization_id,
        asset_id,
        AssetReleaseId::from_uuid(request.param_as::<Uuid>("asset_release_id")?),
    ))
}

pub(super) fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    Ok((idempotency_key, request_id(request)?))
}

pub(super) fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

pub(super) fn mcp_service_profile_acl(request: &BootRequest) -> Result<String> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|value| value.eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE)) {
        return Err(BootError::UnsupportedMediaType(
            "MCP Service profiles require application/vnd.a3s.acl".into(),
        ));
    }
    if request.body().is_empty() {
        return Err(BootError::BadRequest(
            "MCP Service profile ACL body is required".into(),
        ));
    }
    if request.body().len() > MCP_SERVICE_PROFILE_MAX_ACL_BYTES {
        return Err(BootError::PayloadTooLarge(
            "MCP Service profile ACL exceeds 65536 bytes".into(),
        ));
    }
    std::str::from_utf8(request.body())
        .map(str::to_owned)
        .map_err(|_| BootError::BadRequest("MCP Service profile ACL must be valid UTF-8".into()))
}
