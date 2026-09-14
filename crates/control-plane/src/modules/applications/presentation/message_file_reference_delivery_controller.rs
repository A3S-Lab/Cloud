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
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageFileReferenceId, ApplicationMessageId, ApplicationSessionId,
    OrganizationId, ProjectId, Sha256Digest, UserFileId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_message_file_reference_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateApplicationMessageFileReferenceRequest =
                        request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    let content_digest = match Sha256Digest::parse(body.content_digest) {
                        Ok(value) => value,
                        Err(error) => {
                            return application_error_response(
                                ApplicationError::Invalid(error),
                                request_id,
                            );
                        }
                    };
                    match bus
                        .execute(CreateApplicationMessageFileReference {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )
}

pub fn application_message_file_reference_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListApplicationMessageFileReferencesBySession {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-file-references/{reference_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetApplicationMessageFileReference {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
                        Ok(reference) => BootResponse::json(
                            &ApplicationMessageFileReferenceResponse::from(reference),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
