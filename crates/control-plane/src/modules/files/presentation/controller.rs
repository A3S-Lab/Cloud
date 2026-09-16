use super::content_stream::stream_user_file_content;
use super::{
    ExpireUserFileUploadRequest, RecordUserFileScanRequest, ReserveUserFileRequest,
    TombstoneUserFileRequest, UserFileMutationResponse, UserFileQuotaResponse, UserFileResponse,
    USER_FILES_CONTROLLER_PREFIX, USER_FILE_COLLECTION_ROUTE, USER_FILE_CONTENT_ROUTE,
    USER_FILE_EXPIRE_ROUTE, USER_FILE_ITEM_ROUTE, USER_FILE_QUOTA_ROUTE, USER_FILE_SCAN_ROUTE,
    USER_FILE_TOMBSTONE_ROUTE,
};
use crate::modules::files::application::{
    ExpireUserFileUpload, GetUserFile, GetUserFileContent, GetUserFileQuota, ListUserFiles,
    RecordUserFileScan, RecordUserFileUpload, ReserveUserFile, TombstoneUserFile,
    UserFileTransition, DEFAULT_USER_FILE_LIST_LIMIT, MAXIMUM_USER_FILE_LIST_LIMIT,
};
use crate::modules::files::USER_FILE_MAX_BYTES;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, UserFileId};
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_cloud_read_controller,
    organization_tenant_file_write_controller, request_id, request_identity,
    resource_access_evaluator, user_file_access, with_deferred_resource_scope,
    DeferredResourceScope,
};
use a3s_boot::{
    controller, get, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result, RouteDefinition,
};
use std::io::Cursor;
use std::sync::Arc;
use uuid::Uuid;

pub fn user_file_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let reserve_bus = Arc::clone(&bus);
    let content_bus = Arc::clone(&bus);
    let scan_bus = Arc::clone(&bus);
    let expire_bus = Arc::clone(&bus);
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
        .put(USER_FILE_CONTENT_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&content_bus);
            async move {
                require_octet_stream_content_type(&request)?;
                request.validate_with_body_limit(USER_FILE_MAX_BYTES as usize)?;
                let expected_version = expected_version(&request)?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                let user_file_id = UserFileId::from_uuid(request.param_as::<Uuid>("user_file_id")?);
                let actor_principal_id = actor_principal_id(&request)?;
                let access = user_file_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?);
                let reader = Box::pin(Cursor::new(request.into_body()));
                match bus
                    .execute(RecordUserFileUpload {
                        transition: UserFileTransition {
                            organization_id,
                            project_id,
                            user_file_id,
                            expected_version,
                            actor_principal_id,
                            access,
                            idempotency_key,
                            request_id,
                        },
                        reader,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json(&UserFileMutationResponse::from(result)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(USER_FILE_SCAN_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&scan_bus);
            async move {
                let body: RecordUserFileScanRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(RecordUserFileScan {
                        transition: UserFileTransition {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
                        },
                        evidence_digest: body.evidence_digest,
                        decision: body.decision.into(),
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json(&UserFileMutationResponse::from(result)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(USER_FILE_EXPIRE_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&expire_bus);
            async move {
                let body: ExpireUserFileUploadRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(ExpireUserFileUpload(UserFileTransition {
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
    // Nest macros own list/get/content; quota keeps deferred resource admission.
    // Tenant admission stays on the cloud read entry helper (architecture boundary).
    let mut controller = Arc::new(UserFileQueriesController { bus: Arc::clone(&bus) }).controller()?;
    controller = controller.route(with_deferred_resource_scope(
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

#[derive(Debug, Clone)]
struct UserFileQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl UserFileQueriesController {
    #[get("/{organization_id}/projects/{project_id}/user-files", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get(
        "/{organization_id}/projects/{project_id}/user-files/{user_file_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetUserFile {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                user_file_id: UserFileId::from_uuid(request.param_as::<Uuid>("user_file_id")?),
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

    #[get(
        "/{organization_id}/projects/{project_id}/user-files/{user_file_id}/content",
        raw
    )]
    async fn content(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetUserFileContent {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                user_file_id: UserFileId::from_uuid(request.param_as::<Uuid>("user_file_id")?),
                access: user_file_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(content) => stream_user_file_content(
                content.media_type,
                content.size_bytes,
                content.reader,
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
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

fn expected_version(request: &BootRequest) -> Result<u64> {
    let raw = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?;
    let expected_version = raw.parse::<u64>().map_err(|_| {
        BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
    })?;
    if expected_version == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    Ok(expected_version)
}

fn require_octet_stream_content_type(request: &BootRequest) -> Result<()> {
    let content_type = request.header("content-type").unwrap_or_default().trim();
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case("application/octet-stream") {
        return Err(BootError::UnsupportedMediaType(
            "UserFile content requires application/octet-stream".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod nest_macro_user_file_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn user_file_queries_controller_registers_list_get_content_via_nest_macros() {
        let controller = user_file_queries_controller(Arc::new(QueryBus::new()))
            .expect("user file queries nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/user-files"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/user-files/{user_file_id}"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/user-files/{user_file_id}/content"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/user-file-quota"
        }));
    }
}
