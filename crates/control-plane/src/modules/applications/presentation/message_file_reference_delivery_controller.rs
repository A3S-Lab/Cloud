use super::delivery_dto::{
    ApplicationMessageFileReferenceMutationResponse, ApplicationMessageFileReferenceResponse,
    CreateApplicationMessageFileReferenceRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplicationMessageFileReference, GetApplicationMessageFileReference,
    ListApplicationMessageFileReferencesBySession,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageFileReferenceId, ApplicationMessageId, ApplicationSessionId,
    OrganizationId, ProjectId, Sha256Digest, UserFileId,
};
use crate::presentation::{
    OrganizationTenantGuard, resource_access_evaluator, actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_message_file_reference_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageFileReferenceCommandsController { bus }).controller()
}

pub fn application_message_file_reference_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageFileReferenceQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationMessageFileReferenceCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationMessageFileReferenceQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageFileReferenceCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationMessageFileReferenceRequest =
            request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        let content_digest = match Sha256Digest::parse(body.content_digest) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(ApplicationError::Invalid(error), request_id);
            }
        };
        match self
            .bus
            .execute(CreateApplicationMessageFileReference {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                message_id: ApplicationMessageId::from_uuid(body.message_id),
                user_file_id: UserFileId::from_uuid(body.user_file_id),
                content_digest,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                created_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationMessageFileReferenceMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageFileReferenceQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationMessageFileReferencesBySession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(items) => BootResponse::json(
                &items
                    .into_iter()
                    .map(ApplicationMessageFileReferenceResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references/{reference_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationMessageFileReference {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                reference_id: ApplicationMessageFileReferenceId::from_uuid(
                    request.param_as::<Uuid>("reference_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(reference) => {
                BootResponse::json(&ApplicationMessageFileReferenceResponse::from(reference))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_message_file_reference_delivery_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn message_file_reference_controllers_register_scoped_routes_via_nest_macros() {
        let commands = application_message_file_reference_commands_controller(
            Arc::new(CommandBus::new()),
        )
        .expect("message file reference nest command controller");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 1);
        assert_eq!(commands.routes()[0].method(), HttpMethod::Post);

        let queries = application_message_file_reference_queries_controller(
            Arc::new(QueryBus::new()),
        )
        .expect("message file reference nest query controller");
        assert_eq!(queries.routes().len(), 2);
        assert_eq!(
            queries.routes()[1].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references/{reference_id}"
        );
        assert_eq!(
            queries.routes()[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::APPLICATION_WRITE])
        );
    }
}
