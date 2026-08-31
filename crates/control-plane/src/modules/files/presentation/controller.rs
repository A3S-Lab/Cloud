use super::{
    ReserveUserFileRequest, TombstoneUserFileRequest, UserFileMutationResponse,
    UserFileQuotaResponse, UserFileResponse, USER_FILES_CONTROLLER_PREFIX,
    USER_FILE_COLLECTION_ROUTE, USER_FILE_ITEM_ROUTE, USER_FILE_QUOTA_ROUTE,
    USER_FILE_TOMBSTONE_ROUTE,
};
use crate::modules::files::application::{
    GetUserFile, GetUserFileQuota, ListUserFiles, ReserveUserFile, TombstoneUserFile,
    UserFileTransition, DEFAULT_USER_FILE_LIST_LIMIT, MAXIMUM_USER_FILE_LIST_LIMIT,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, UserFileId};
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_cloud_read_controller,
    organization_tenant_file_write_controller, request_id, request_identity,
    resource_access_evaluator, user_file_access, with_deferred_resource_scope,
    DeferredResourceScope,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn user_file_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let reserve_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(USER_FILES_CONTROLLER_PREFIX)?
        .post(USER_FILE_COLLECTION_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&reserve_bus);
            async move {
                let body: ReserveUserFileRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(ReserveUserFile {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        admission_acl: body.admission_acl,
                        actor_principal_id: actor_principal_id(&request)?,
                        access: user_file_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?),
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json_with_status(
                        if result.replayed { 200 } else { 201 },
                        &UserFileMutationResponse::from(result),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(USER_FILE_TOMBSTONE_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: TombstoneUserFileRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(TombstoneUserFile(UserFileTransition {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        user_file_id: UserFileId::from_uuid(
                            request.param_as::<Uuid>("user_file_id")?,
                        ),
                        expected_version: body.expected_version,
                        actor_principal_id: actor_principal_id(&request)?,
                        access: user_file_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?),
                        idempotency_key,
                        request_id,
                    }))
                    .await?
                {
                    Ok(result) => BootResponse::json(&UserFileMutationResponse::from(result)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?;
    organization_tenant_file_write_controller(controller)
}

pub fn user_file_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(USER_FILES_CONTROLLER_PREFIX)?
        .get(USER_FILE_COLLECTION_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&list_bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListUserFiles {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        limit: Some(list_limit(&request)?),
                        access: user_file_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?),
                    })
                    .await?
                {
                    Ok(files) => BootResponse::json(
                        &files
                            .into_iter()
                            .map(UserFileResponse::from)
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .get(USER_FILE_ITEM_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&get_bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetUserFile {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        user_file_id: UserFileId::from_uuid(
                            request.param_as::<Uuid>("user_file_id")?,
                        ),
                        access: user_file_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?),
                    })
                    .await?
                {
                    Ok(file) => BootResponse::json(&UserFileResponse::from(file)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(USER_FILE_QUOTA_ROUTE, move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetUserFileQuota {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            access: user_file_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(quota) => BootResponse::json(&UserFileQuotaResponse::from(quota)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?,
            DeferredResourceScope::Any,
        )?)?;
    organization_tenant_cloud_read_controller(controller)
}

fn list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_USER_FILE_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_USER_FILE_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_USER_FILE_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}
