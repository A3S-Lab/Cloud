use super::delivery_dto::{
    ApplicationInvocationMutationResponse, ApplicationInvocationResponse,
    ApplicationMessageResponse, ApplicationSessionMutationResponse, ApplicationSessionResponse,
    OpenApplicationSessionRequest, RequestApplicationInvocationRequest,
};
use crate::modules::applications::application::{
    AdmitApplicationInvocation, AdmitApplicationSession, GetApplicationInvocation,
    GetApplicationSession, ReplayApplicationSession, DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT,
    MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
};
use crate::modules::applications::domain::ApplicationResponseMode;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, ProjectId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, request_id, request_identity,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn application_delivery_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let open_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions",
            move |request: BootRequest| {
                let bus = Arc::clone(&open_bus);
                async move {
                    let body: OpenApplicationSessionRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(AdmitApplicationSession {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            application_id: ApplicationId::from_uuid(
                                request.param_as::<Uuid>("application_id")?,
                            ),
                            release_id: ApplicationReleaseId::from_uuid(body.release_id),
                            initial_variables: body.initial_variables,
                            actor_principal_id: actor_principal_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: RequestApplicationInvocationRequest =
                        request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let response_mode = ApplicationResponseMode::parse(&body.response_mode)
                        .map_err(BootError::BadRequest)?;
                    match bus
                        .execute(AdmitApplicationInvocation {
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
                            ontology_id: OntologyId::from_uuid(body.ontology_id),
                            ontology_revision_id: OntologyRevisionId::from_uuid(
                                body.ontology_revision_id,
                            ),
                            environment_id: body.environment_id.map(EnvironmentId::from_uuid),
                            response_mode,
                            input: body.input,
                            timeout_seconds: body.timeout_seconds,
                            actor_principal_id: actor_principal_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )
}

pub fn application_delivery_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let session_bus = Arc::clone(&bus);
    let invocation_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&session_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus.execute(GetApplicationSession {
                        organization_id: OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        application_id: ApplicationId::from_uuid(request.param_as::<Uuid>("application_id")?),
                        session_id: ApplicationSessionId::from_uuid(request.param_as::<Uuid>("session_id")?),
                        actor_principal_id: actor_principal_id(&request)?,
                        resource_access: resource_access_evaluator(&request.require_auth_principal()?)?,
                    }).await? {
                        Ok(result) => {
                            BootResponse::json(&ApplicationSessionResponse::from(result.session))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&invocation_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus.execute(GetApplicationInvocation {
                        organization_id: OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        application_id: ApplicationId::from_uuid(request.param_as::<Uuid>("application_id")?),
                        session_id: ApplicationSessionId::from_uuid(request.param_as::<Uuid>("session_id")?),
                        invocation_id: ApplicationInvocationId::from_uuid(request.param_as::<Uuid>("invocation_id")?),
                        actor_principal_id: actor_principal_id(&request)?,
                        resource_access: resource_access_evaluator(&request.require_auth_principal()?)?,
                    }).await? {
                        Ok(invocation) => BootResponse::json(&ApplicationInvocationResponse::from(invocation)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/messages",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = message_limit(&request)?;
                    match bus.execute(ReplayApplicationSession {
                        organization_id: OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
                        project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                        application_id: ApplicationId::from_uuid(request.param_as::<Uuid>("application_id")?),
                        session_id: ApplicationSessionId::from_uuid(request.param_as::<Uuid>("session_id")?),
                        after_sequence: request
                            .optional_query_value_as::<u64>("afterSequence")?
                            .unwrap_or_default(),
                        limit: Some(limit),
                        actor_principal_id: actor_principal_id(&request)?,
                        resource_access: resource_access_evaluator(&request.require_auth_principal()?)?,
                    }).await? {
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
            },
        )
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
