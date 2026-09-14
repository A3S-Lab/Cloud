use super::delivery_dto::{
    ApplicationAnnotationMutationResponse, ApplicationAnnotationResponse,
    ApplicationFeedbackMutationResponse, ApplicationFeedbackResponse,
    CreateApplicationAnnotationRequest, CreateApplicationFeedbackRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplicationAnnotation, CreateApplicationFeedback, GetApplicationAnnotation,
    GetApplicationFeedback, ListApplicationAnnotationsBySession, ListApplicationFeedbackBySession,
};
use crate::modules::applications::domain::ApplicationFeedbackRating;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationFeedbackId, ApplicationId, ApplicationMessageId,
    ApplicationSessionId, OrganizationId, ProjectId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    AUTH_SCOPES_METADATA, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_feedback_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let feedback_bus = Arc::clone(&bus);
    let annotation_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
            move |request: BootRequest| {
                let bus = Arc::clone(&feedback_bus);
                async move {
                    let body: CreateApplicationFeedbackRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    let rating = ApplicationFeedbackRating::parse(&body.rating)
                        .map_err(BootError::BadRequest)?;
                    match bus
                        .execute(CreateApplicationFeedback {
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
                            source_message_id: body
                                .source_message_id
                                .map(ApplicationMessageId::from_uuid),
                            rating,
                            comment: body.comment,
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
                            &ApplicationFeedbackMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
            move |request: BootRequest| {
                let bus = Arc::clone(&annotation_bus);
                async move {
                    let body: CreateApplicationAnnotationRequest = request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(CreateApplicationAnnotation {
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
                            source_message_id: body
                                .source_message_id
                                .map(ApplicationMessageId::from_uuid),
                            content: body.content,
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
                            &ApplicationAnnotationMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn application_feedback_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_feedback_bus = Arc::clone(&bus);
    let list_feedback_bus = Arc::clone(&bus);
    let get_annotation_bus = Arc::clone(&bus);
    let list_annotation_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_feedback_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListApplicationFeedbackBySession {
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
                                .map(ApplicationFeedbackResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks/{feedback_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_feedback_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetApplicationFeedback {
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
                            feedback_id: ApplicationFeedbackId::from_uuid(
                                request.param_as::<Uuid>("feedback_id")?,
                            ),
                            actor_principal_id: actor_principal_id(&request)?,
                            access: application_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(feedback) => {
                            BootResponse::json(&ApplicationFeedbackResponse::from(feedback))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_annotation_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListApplicationAnnotationsBySession {
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
                                .map(ApplicationAnnotationResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations/{annotation_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_annotation_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetApplicationAnnotation {
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
                            annotation_id: ApplicationAnnotationId::from_uuid(
                                request.param_as::<Uuid>("annotation_id")?,
                            ),
                            actor_principal_id: actor_principal_id(&request)?,
                            access: application_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(annotation) => {
                            BootResponse::json(&ApplicationAnnotationResponse::from(annotation))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
