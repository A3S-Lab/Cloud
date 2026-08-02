use crate::modules::assets::application::commands::ReceiveAssetGitPack;
use crate::modules::assets::application::queries::{
    AdvertiseAssetGitRepository, UploadAssetGitPack,
};
use crate::modules::assets::domain::AssetGitService;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

const BASE_PATH: &str = "/organizations";
const REPOSITORY_PATH: &str = "/{organization_id}/assets/{asset_id}/git";

pub fn advertisement_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new(BASE_PATH)?
        .with_guard(OrganizationTenantGuard)
        .get(
            format!("{REPOSITORY_PATH}/info/refs"),
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let service = advertisement_service(&request)?;
                    require_scope(&request, service_scope(service))?;
                    let (organization_id, asset_id) = repository_ids(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(AdvertiseAssetGitRepository {
                            organization_id,
                            asset_id,
                            service,
                        })
                        .await?
                    {
                        Ok(body) => Ok(git_response(body, service.advertisement_media_type())),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn upload_pack_controller(
    bus: Arc<QueryBus>,
    maximum_body_bytes: usize,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new(BASE_PATH)?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .post(
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
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(UploadAssetGitPack {
                            organization_id,
                            asset_id,
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
        )
}

pub fn receive_pack_controller(
    bus: Arc<CommandBus>,
    maximum_body_bytes: usize,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new(BASE_PATH)?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ASSET_WRITE])?
        .post(
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
                    let request_id = request_id(&request)?;
                    let actor_id = actor_id(&request)?;
                    match bus
                        .execute(ReceiveAssetGitPack {
                            organization_id,
                            asset_id,
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
        )
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

fn service_scope(service: AssetGitService) -> &'static str {
    match service {
        AssetGitService::UploadPack => ApiTokenScope::CLOUD_READ,
        AssetGitService::ReceivePack => ApiTokenScope::ASSET_WRITE,
    }
}

fn require_scope(request: &BootRequest, scope: &str) -> Result<()> {
    if request.require_auth_principal()?.has_scope(scope) {
        return Ok(());
    }
    Err(BootError::Forbidden(
        "authenticated token does not have the required scope".into(),
    ))
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

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

fn actor_id(request: &BootRequest) -> Result<Uuid> {
    Uuid::parse_str(request.require_auth_principal()?.subject()).map_err(|error| {
        BootError::Internal(format!("authenticated token identity is invalid: {error}"))
    })
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
