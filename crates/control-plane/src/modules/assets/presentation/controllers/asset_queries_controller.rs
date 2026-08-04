use crate::modules::assets::application::queries::{
    GetAsset, GetAssetRelease, ListAssetReleases, ListAssets, SelectAssetRelease,
};
use crate::modules::assets::presentation::dto::{AssetReleaseResponse, AssetResponse};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetReleaseSelectionQuery {
    version: Option<String>,
}

pub fn asset_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_assets = Arc::clone(&bus);
    let get_assets = Arc::clone(&bus);
    let list_releases = Arc::clone(&bus);
    let get_releases = Arc::clone(&bus);
    let select_releases = bus;
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get("/{organization_id}/assets", move |request: BootRequest| {
            let bus = Arc::clone(&list_assets);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let request_id = request_id(&request)?;
                match bus.execute(ListAssets { organization_id }).await? {
                    Ok(assets) => BootResponse::json(
                        &assets
                            .into_iter()
                            .map(AssetResponse::from)
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .get(
            "/{organization_id}/assets/{asset_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_assets);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAsset {
                            organization_id,
                            asset_id,
                        })
                        .await?
                    {
                        Ok(asset) => BootResponse::json(&AssetResponse::from(asset)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/assets/{asset_id}/releases",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_releases);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListAssetReleases {
                            organization_id,
                            asset_id,
                        })
                        .await?
                    {
                        Ok(releases) => BootResponse::json(
                            &releases
                                .into_iter()
                                .map(AssetReleaseResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_releases);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let asset_release_id =
                        AssetReleaseId::from_uuid(request.param_as::<Uuid>("asset_release_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAssetRelease {
                            organization_id,
                            asset_id,
                            asset_release_id,
                        })
                        .await?
                    {
                        Ok(release) => BootResponse::json(&AssetReleaseResponse::from(release)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/assets/{asset_id}/release-selection",
            move |request: BootRequest| {
                let bus = Arc::clone(&select_releases);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let query: AssetReleaseSelectionQuery = request.query()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(SelectAssetRelease {
                            organization_id,
                            asset_id,
                            requested_version: query.version,
                        })
                        .await?
                    {
                        Ok(release) => BootResponse::json(&AssetReleaseResponse::from(release)),
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

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
