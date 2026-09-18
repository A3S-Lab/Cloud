use super::delivery_process_drain::DeliveryProcessDrain;
use super::delivery_dto::{
    ApplicationExpectedVersionRequest, ApplicationInvocationCancellationResponse,
    ApplicationInvocationMutationResponse, ApplicationSessionMutationResponse,
    OpenApplicationSessionRequest, RequestApplicationInvocationRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    AdmitApplicationInvocation, AdmitApplicationSession, CancelApplicationInvocation,
    CloseApplicationSession,
};
use crate::modules::applications::domain::ApplicationResponseMode;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, ProjectId,
};
use crate::presentation::{
    OrganizationTenantGuard, resource_access_evaluator, 
    actor_principal_id, application_error_response, request_identity,
};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn exact_application_access(
    request: &BootRequest,
) -> Result<(
    crate::modules::applications::ApplicationAccess,
    ProjectId,
    ApplicationId,
)> {
    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
    let application_id = ApplicationId::from_uuid(request.param_as::<Uuid>("application_id")?);
    let access = application_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?);
    if !access.exact_application_is_authorized(project_id, application_id) {
        return Err(BootError::NotFound("Application not found".into()));
    }
    Ok((access, project_id, application_id))
}

/// Authenticated published-application delivery under `/delivery` (APP0.3-C5).
///
/// Requires `application:invoke` plus an exact Application ResourceGrant (Rule 8).
/// Project-only grants fail closed in the controller even if CQRS would accept them.
pub fn authenticated_application_delivery_commands_controller(
    bus: Arc<CommandBus>,
    drain: DeliveryProcessDrain,
) -> Result<ControllerDefinition> {
    Arc::new(AuthenticatedApplicationDeliveryCommandsController { bus, drain }).controller()
}

#[derive(Debug, Clone)]
struct AuthenticatedApplicationDeliveryCommandsController {
    bus: Arc<CommandBus>,
    drain: DeliveryProcessDrain,
}

#[controller("/delivery")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_INVOKE])]
impl AuthenticatedApplicationDeliveryCommandsController {
    #[post(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions",
        raw
    )]
    async fn open_session(&self, request: BootRequest) -> Result<BootResponse> {
        self.drain.refuse_new_admission()?;
        let body: OpenApplicationSessionRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(AdmitApplicationSession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                application_id,
                release_id: ApplicationReleaseId::from_uuid(body.release_id),
                initial_variables: body.initial_variables,
                actor_principal_id: actor_principal_id(&request)?,
                access,
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
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations",
        raw
    )]
    async fn request_invocation(&self, request: BootRequest) -> Result<BootResponse> {
        self.drain.refuse_new_admission()?;
        let body: RequestApplicationInvocationRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let response_mode =
            ApplicationResponseMode::parse(&body.response_mode).map_err(BootError::BadRequest)?;
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(AdmitApplicationInvocation {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                application_id,
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
                access,
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
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/close",
        raw
    )]
    async fn close_session(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ApplicationExpectedVersionRequest = request.json_with_content_type()?;
        let (_, request_id) = request_identity(&request)?;
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(CloseApplicationSession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                application_id,
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                expected_version: body.expected_version,
                actor_principal_id: actor_principal_id(&request)?,
                access,
                closed_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json(&ApplicationSessionMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/cancel",
        raw
    )]
    async fn cancel_invocation(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ApplicationExpectedVersionRequest = request.json_with_content_type()?;
        let (_, request_id) = request_identity(&request)?;
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(CancelApplicationInvocation {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                application_id,
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                invocation_id: ApplicationInvocationId::from_uuid(
                    request.param_as::<Uuid>("invocation_id")?,
                ),
                expected_version: body.expected_version,
                actor_principal_id: actor_principal_id(&request)?,
                access,
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

#[cfg(test)]
mod nest_macro_authenticated_delivery_controller_tests {
    use super::*;

    #[test]
    fn authenticated_delivery_registers_invoke_scoped_routes_via_nest_macros() {
        let controller = authenticated_application_delivery_commands_controller(
            Arc::new(CommandBus::new()),
            DeliveryProcessDrain::default(),
        )
        .expect("authenticated delivery controller");
        assert_eq!(controller.prefix(), "/delivery");
        assert_eq!(controller.routes().len(), 4);
        assert_eq!(
            controller.routes()[0].path(),
            "/delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions"
        );
        assert_eq!(
            controller.routes()[1].path(),
            "/delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations"
        );
        assert_eq!(
            controller.routes()[2].path(),
            "/delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/close"
        );
        assert_eq!(
            controller.routes()[3].path(),
            "/delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/cancel"
        );
        assert!(
            controller
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .is_some()
        );
    }
}

