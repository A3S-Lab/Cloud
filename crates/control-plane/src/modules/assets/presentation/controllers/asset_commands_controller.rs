use super::asset_request::{asset_ids, asset_release_ids, organization_id};
use crate::modules::assets::application::commands::{
    ArchiveAsset, CreateAsset, CreateAssetRelease, YankAssetRelease,
};
use crate::modules::assets::presentation::dto::{
    AssetReleaseResponse, AssetResponse, CreateAssetReleaseRequest, CreateAssetRequest,
};
use crate::presentation::{
    DeferredResourceScope, application_error_response, asset_access,
    organization_tenant_asset_write_controller, request_identity, resource_access_evaluator,
    with_deferred_resource_scope,
};
use a3s_boot::{
    controller, post, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    RouteDefinition,
};
use std::sync::Arc;

pub fn asset_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create; archive/release/yank keep deferred resource admission.
    // Tenant admission stays on the Assets entry helper (architecture boundary).
    let archive_assets = Arc::clone(&bus);
    let create_releases = Arc::clone(&bus);
    let yank_releases = Arc::clone(&bus);
    let mut controller = Arc::new(AssetCommandsController { bus }).controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/assets/{asset_id}/archive",
            move |request: BootRequest| {
                let bus = Arc::clone(&archive_assets);
                async move {
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ArchiveAsset {
                            organization_id,
                            asset_id,
                            access,
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
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/assets/{asset_id}/releases",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_releases);
                async move {
                    let body: CreateAssetReleaseRequest = request.json_with_content_type()?;
                    let (organization_id, asset_id) = asset_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateAssetRelease {
                            organization_id,
                            asset_id,
                            access,
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
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank",
            move |request: BootRequest| {
                let bus = Arc::clone(&yank_releases);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(YankAssetRelease {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            access,
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
    )?)?;
    organization_tenant_asset_write_controller(controller)
}

#[derive(Debug, Clone)]
struct AssetCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
impl AssetCommandsController {
    #[post("/{organization_id}/assets", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateAssetRequest = request.json_with_content_type()?;
        let organization_id = organization_id(&request)?;
        let access = asset_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateAsset {
                organization_id,
                access,
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
}

#[cfg(test)]
mod nest_macro_asset_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn asset_commands_controller_registers_create_via_nest_macros() {
        let controller = asset_commands_controller(Arc::new(CommandBus::new()))
            .expect("asset commands nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path() == "/organizations/{organization_id}/assets"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/assets/{asset_id}/archive"
        }));
    }
}
