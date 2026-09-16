use super::delivery_dto::{
    ApplicationExpectedVersionRequest, ApplicationInvocationCancellationResponse,
    ApplicationInvocationMutationResponse, ApplicationInvocationResponse,
    ApplicationMessageResponse, ApplicationSessionMutationResponse,
    ApplicationSessionReplayResponse, ApplicationSessionResponse, OpenApplicationSessionRequest,
    RequestApplicationInvocationRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    AdmitApplicationInvocation, AdmitApplicationSession, CancelApplicationInvocation,
    CloseApplicationSession, DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT, GetApplicationInvocation,
    GetApplicationSession, MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT, ReplayApplicationSession,
};
use crate::modules::applications::domain::ApplicationResponseMode;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, ProjectId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, request_id, request_identity,
};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_delivery_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationDeliveryCommandsController { bus }).controller()
}

pub fn application_delivery_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(ApplicationDeliveryQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationDeliveryCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationDeliveryQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationDeliveryCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions",
        raw
    )]
    async fn open_session(&self, request: BootRequest) -> Result<BootResponse> {
        let body: OpenApplicationSessionRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(AdmitApplicationSession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                release_id: ApplicationReleaseId::from_uuid(body.release_id),
                initial_variables: body.initial_variables,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationSessionMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations",
        raw
    )]
    async fn request_invocation(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RequestApplicationInvocationRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let response_mode =
            ApplicationResponseMode::parse(&body.response_mode).map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(AdmitApplicationInvocation {
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
                ontology_id: OntologyId::from_uuid(body.ontology_id),
                ontology_revision_id: OntologyRevisionId::from_uuid(body.ontology_revision_id),
                environment_id: body.environment_id.map(EnvironmentId::from_uuid),
                response_mode,
                input: body.input,
                timeout_seconds: body.timeout_seconds,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationInvocationMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/close",
        raw
    )]
    async fn close_session(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ApplicationExpectedVersionRequest = request.json_with_content_type()?;
        let (_, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CloseApplicationSession {
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
                expected_version: body.expected_version,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                closed_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json(&ApplicationSessionMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/cancel",
        raw
    )]
    async fn cancel_invocation(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ApplicationExpectedVersionRequest = request.json_with_content_type()?;
        let (_, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CancelApplicationInvocation {
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
                invocation_id: ApplicationInvocationId::from_uuid(
                    request.param_as::<Uuid>("invocation_id")?,
                ),
                expected_version: body.expected_version,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                BootResponse::json(&ApplicationInvocationCancellationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationDeliveryQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}",
        raw
    )]
    async fn get_session(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationSession {
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
            Ok(result) => BootResponse::json(&ApplicationSessionResponse::from(result.session)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/replay",
        raw
    )]
    async fn replay_session(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = message_limit(&request)?;
        match self
            .bus
            .execute(ReplayApplicationSession {
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
                after_sequence: request
                    .optional_query_value_as::<u64>("afterSequence")?
                    .unwrap_or_default(),
                limit: Some(limit),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(result) => BootResponse::json(&ApplicationSessionReplayResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}",
        raw
    )]
    async fn get_invocation(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationInvocation {
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
                invocation_id: ApplicationInvocationId::from_uuid(
                    request.param_as::<Uuid>("invocation_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(invocation) => {
                BootResponse::json(&ApplicationInvocationResponse::from(invocation))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/messages",
        raw
    )]
    async fn list_messages(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = message_limit(&request)?;
        match self
            .bus
            .execute(ReplayApplicationSession {
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
                after_sequence: request
                    .optional_query_value_as::<u64>("afterSequence")?
                    .unwrap_or_default(),
                limit: Some(limit),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(result) => BootResponse::json(
                &result
                    .messages
                    .into_iter()
                    .map(ApplicationMessageResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn message_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT);
    if limit == 0 || limit > MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT}"
        )));
    }
    Ok(limit)
}

#[cfg(test)]
mod nest_macro_application_delivery_controller_tests {
    use super::*;

    #[test]
    fn application_delivery_registers_write_scoped_routes_via_nest_macros() {
        let commands = application_delivery_commands_controller(Arc::new(CommandBus::new()))
            .expect("delivery commands");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 4);
        assert!(commands.metadata().get(AUTH_SCOPES_METADATA).is_some());

        let queries = application_delivery_queries_controller(Arc::new(QueryBus::new()))
            .expect("delivery queries");
        assert_eq!(queries.prefix(), "/organizations");
        assert_eq!(queries.routes().len(), 4);
        assert_eq!(
            queries.routes()[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}"
        );
        assert_eq!(
            queries.routes()[1].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/replay"
        );
        assert_eq!(
            queries.routes()[3].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/messages"
        );
    }
}

