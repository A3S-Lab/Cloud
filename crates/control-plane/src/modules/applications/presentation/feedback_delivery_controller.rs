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
    controller, get, metadata, post, use_guard, BootError, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_feedback_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationFeedbackCommandsController { bus }).controller()
}

pub fn application_feedback_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(ApplicationFeedbackQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationFeedbackCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationFeedbackQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationFeedbackCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
        raw
    )]
    async fn create_feedback(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationFeedbackRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        let rating =
            ApplicationFeedbackRating::parse(&body.rating).map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(CreateApplicationFeedback {
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

    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
        raw
    )]
    async fn create_annotation(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationAnnotationRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CreateApplicationAnnotation {
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
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationFeedbackQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
        raw
    )]
    async fn list_feedbacks(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationFeedbackBySession {
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
                    .map(ApplicationFeedbackResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks/{feedback_id}",
        raw
    )]
    async fn get_feedback(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationFeedback {
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
            Ok(feedback) => BootResponse::json(&ApplicationFeedbackResponse::from(feedback)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
        raw
    )]
    async fn list_annotations(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationAnnotationsBySession {
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
                    .map(ApplicationAnnotationResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations/{annotation_id}",
        raw
    )]
    async fn get_annotation(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationAnnotation {
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
}

#[cfg(test)]
mod nest_macro_feedback_delivery_controller_tests {
    use super::*;

    #[test]
    fn feedback_controllers_register_scoped_routes_via_nest_macros() {
        let commands = application_feedback_commands_controller(Arc::new(CommandBus::new()))
            .expect("commands");
        let queries =
            application_feedback_queries_controller(Arc::new(QueryBus::new())).expect("queries");
        assert_routes_contain(
            &commands,
            &[
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
                ),
            ],
        );
        assert_routes_contain(
            &queries,
            &[
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/feedbacks/{feedback_id}",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/annotations/{annotation_id}",
                ),
            ],
        );
    }

    fn assert_routes_contain(controller: &ControllerDefinition, expected: &[(&str, &str)]) {
        let routes = controller.routes();
        for (method, path) in expected {
            assert!(
                routes.iter().any(|route| {
                    route.method().as_str().eq_ignore_ascii_case(method) && route.path() == *path
                }),
                "missing {method} {path}; have {:?}",
                routes
                    .iter()
                    .map(|route| format!("{} {}", route.method().as_str(), route.path()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
