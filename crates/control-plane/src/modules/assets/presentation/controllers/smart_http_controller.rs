use crate::modules::assets::application::commands::ReceiveAssetGitPack;
use crate::modules::assets::application::queries::{
    AdvertiseAssetGitRepository, UploadAssetGitPack,
};
use crate::modules::assets::domain::AssetGitService;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use crate::presentation::{
    actor_principal_id, application_error_response, asset_access,
    organization_tenant_asset_write_controller, organization_tenant_cloud_read_controller,
    request_id, require_asset_write_scope, require_cloud_read_scope, resource_access_evaluator,
    with_deferred_resource_scope, DeferredResourceScope, OrganizationTenantGuard,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

const BASE_PATH: &str = "/organizations";
const REPOSITORY_PATH: &str = "/{organization_id}/assets/{asset_id}/git";

pub fn advertisement_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new(BASE_PATH)?
        .with_guard(OrganizationTenantGuard)
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                format!("{REPOSITORY_PATH}/info/refs"),
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
                    async move {
                        let service = advertisement_service(&request)?;
                        require_service_scope(&request, service)?;
                        let (organization_id, asset_id) = repository_ids(&request)?;
                        let access = asset_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?);
                        let request_id = request_id(&request)?;
                        match bus
                            .execute(AdvertiseAssetGitRepository {
                                organization_id,
                                asset_id,
                                service,
                                access,
                            })
                            .await?
                        {
                            Ok(body) => Ok(git_response(body, service.advertisement_media_type())),
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Any,
        )?)
}

pub fn upload_pack_controller(
    bus: Arc<QueryBus>,
    maximum_body_bytes: usize,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new(BASE_PATH)?.route(with_deferred_resource_scope(
        RouteDefinition::post(
            format!("{REPOSITORY_PATH}/git-upload-pack"),
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    validate_rpc_request(
                        &request,
                        AssetGitService::UploadPack,
                        maximum_body_bytes,
                    )?;
                    let (organization_id, asset_id) = repository_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(UploadAssetGitPack {
                            organization_id,
                            asset_id,
                            access,
                            body: request.body().to_vec(),
                        })
                        .await?
                    {
                        Ok(response) => Ok(git_response(
                            response.body,
                            AssetGitService::UploadPack.result_media_type(),
                        )),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Any,
    )?)?;
    organization_tenant_cloud_read_controller(controller)
}

pub fn receive_pack_controller(
    bus: Arc<CommandBus>,
    maximum_body_bytes: usize,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new(BASE_PATH)?.route(with_deferred_resource_scope(
        RouteDefinition::post(
            format!("{REPOSITORY_PATH}/git-receive-pack"),
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    validate_rpc_request(
                        &request,
                        AssetGitService::ReceivePack,
                        maximum_body_bytes,
                    )?;
                    let (organization_id, asset_id) = repository_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let request_id = request_id(&request)?;
                    let actor_id = actor_principal_id(&request)?.as_uuid();
                    match bus
                        .execute(ReceiveAssetGitPack {
                            organization_id,
                            asset_id,
                            access,
                            actor_id,
                            request_id,
                            body: request.body().to_vec(),
                        })
                        .await?
                    {
                        Ok(response) => Ok(git_response(
                            response.body,
                            AssetGitService::ReceivePack.result_media_type(),
                        )),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Any,
    )?)?;
    organization_tenant_asset_write_controller(controller)
}

fn advertisement_service(request: &BootRequest) -> Result<AssetGitService> {
    let pairs = request.query_pairs()?;
    if pairs.len() != 1 || pairs[0].0 != "service" {
        return Err(BootError::BadRequest(
            "Git advertisement requires exactly one service query parameter".into(),
        ));
    }
    match pairs[0].1.as_str() {
        "git-upload-pack" => Ok(AssetGitService::UploadPack),
        "git-receive-pack" => Ok(AssetGitService::ReceivePack),
        _ => Err(BootError::BadRequest(
            "Git advertisement service is unsupported".into(),
        )),
    }
}

fn require_service_scope(request: &BootRequest, service: AssetGitService) -> Result<()> {
    match service {
        AssetGitService::UploadPack => require_cloud_read_scope(request),
        AssetGitService::ReceivePack => require_asset_write_scope(request),
    }
}

fn validate_rpc_request(
    request: &BootRequest,
    service: AssetGitService,
    maximum_body_bytes: usize,
) -> Result<()> {
    let content_type = request.header("content-type").unwrap_or_default().trim();
    if !content_type.eq_ignore_ascii_case(service.request_media_type()) {
        return Err(BootError::UnsupportedMediaType(
            "Git RPC content type does not match its service".into(),
        ));
    }
    if request.body().is_empty() {
        return Err(BootError::BadRequest("Git RPC body is required".into()));
    }
    if request.body().len() > maximum_body_bytes {
        return Err(BootError::PayloadTooLarge(
            "Git RPC body exceeds its configured bound".into(),
        ));
    }
    Ok(())
}

fn repository_ids(request: &BootRequest) -> Result<(OrganizationId, AssetId)> {
    Ok((
        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
        AssetId::from_uuid(request.param_as::<Uuid>("asset_id")?),
    ))
}

fn git_response(body: Vec<u8>, content_type: &str) -> BootResponse {
    BootResponse::new(200, body)
        .with_header("content-type", content_type)
        .with_header("cache-control", "no-cache, max-age=0, must-revalidate")
        .with_header("pragma", "no-cache")
        .with_header("expires", "Fri, 01 Jan 1980 00:00:00 GMT")
        .with_header("x-content-type-options", "nosniff")
        .with_header("x-a3s-api-envelope", "1")
}
