use crate::modules::assets::application::commands::{
    ArchiveAsset, CreateAsset, CreateAssetRelease, YankAssetRelease,
};
use crate::modules::assets::presentation::dto::{
    AssetReleaseResponse, AssetResponse, CreateAssetReleaseRequest, CreateAssetRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn asset_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_assets = Arc::clone(&bus);
    let archive_assets = Arc::clone(&bus);
    let create_releases = Arc::clone(&bus);
    let yank_releases = bus;
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ASSET_WRITE])?
        .post("/{organization_id}/assets", move |request: BootRequest| {
            let bus = Arc::clone(&create_assets);
            async move {
                let body: CreateAssetRequest = request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(CreateAsset {
                        organization_id,
                        name: body.name,
                        kind: body.kind,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(write) => {
                        let status = if write.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(status, &AssetResponse::from(write))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(
            "/{organization_id}/assets/{asset_id}/archive",
            move |request: BootRequest| {
                let bus = Arc::clone(&archive_assets);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ArchiveAsset {
                            organization_id,
                            asset_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(write) => BootResponse::json(&AssetResponse::from(write)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/assets/{asset_id}/releases",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_releases);
                async move {
                    let body: CreateAssetReleaseRequest = request.json_with_content_type()?;
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateAssetRelease {
                            organization_id,
                            asset_id,
                            version: body.version,
                            commit_sha: body.commit_sha,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(write) => {
                            let status = if write.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(
                                status,
                                &AssetReleaseResponse::from(write),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank",
            move |request: BootRequest| {
                let bus = Arc::clone(&yank_releases);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let asset_release_id =
                        AssetReleaseId::from_uuid(request.param_as::<Uuid>("asset_release_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(YankAssetRelease {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(write) => BootResponse::json(&AssetReleaseResponse::from(write)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

fn asset_ids(request: &BootRequest) -> Result<(OrganizationId, AssetId)> {
    Ok((
        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
        AssetId::from_uuid(request.param_as::<Uuid>("asset_id")?),
    ))
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
