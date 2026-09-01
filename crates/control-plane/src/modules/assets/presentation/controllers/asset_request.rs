use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use a3s_boot::{BootRequest, Result};
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
