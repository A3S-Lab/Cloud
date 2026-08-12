use super::asset_request::{asset_ids, asset_release_ids, organization_id, request_identity};
use crate::modules::assets::application::commands::{
    ArchiveAsset, CreateAsset, CreateAssetRelease, YankAssetRelease,
};
use crate::modules::assets::presentation::dto::{
    AssetReleaseResponse, AssetResponse, CreateAssetReleaseRequest, CreateAssetRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

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
                let organization_id = organization_id(&request)?;
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
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/assets/{asset_id}/archive",
                move |request: BootRequest| {
                    let bus = Arc::clone(&archive_assets);
                    async move {
                        let (organization_id, asset_id) = asset_ids(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(ArchiveAsset {
                                organization_id,
                                asset_id,
                                resource_access,
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
            )?,
            DeferredResourceScope::Any,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/assets/{asset_id}/releases",
                move |request: BootRequest| {
                    let bus = Arc::clone(&create_releases);
                    async move {
                        let body: CreateAssetReleaseRequest = request.json_with_content_type()?;
                        let (organization_id, asset_id) = asset_ids(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(CreateAssetRelease {
                                organization_id,
                                asset_id,
                                resource_access,
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
            )?,
            DeferredResourceScope::Any,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank",
                move |request: BootRequest| {
                    let bus = Arc::clone(&yank_releases);
                    async move {
                        let (organization_id, asset_id, asset_release_id) =
                            asset_release_ids(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(YankAssetRelease {
                                organization_id,
                                asset_id,
                                asset_release_id,
                                resource_access,
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
            )?,
            DeferredResourceScope::Any,
        )?)
}
