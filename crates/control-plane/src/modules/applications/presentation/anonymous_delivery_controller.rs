use super::delivery_process_drain::DeliveryProcessDrain;
use super::delivery_dto::{
    ApplicationInvocationCancellationResponse, ApplicationInvocationMutationResponse,
    ApplicationSessionMutationResponse, CancelAnonymousApplicationInvocationRequest,
    CloseAnonymousApplicationSessionRequest, OpenAnonymousApplicationSessionRequest,
    RequestAnonymousApplicationInvocationRequest,
};
use crate::modules::applications::application::{
    CancelAnonymousApplicationInvocation, CloseAnonymousApplicationSession,
    OpenAnonymousApplicationSession, RequestAnonymousApplicationInvocation,
};
use crate::modules::applications::domain::ApplicationResponseMode;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, ProjectId, Sha256Digest,
};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    controller, metadata, post, BootError, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Public anonymous delivery admission over opaque credential lookup keys.
///
/// Mirrors Automation webhook public metadata: no organization tenant guard and
/// no `application:write` scope. CQRS authorizes by `credential_lookup_key`.
pub fn anonymous_application_delivery_commands_controller(
    bus: Arc<CommandBus>,
    drain: DeliveryProcessDrain,
) -> Result<ControllerDefinition> {
    Arc::new(AnonymousApplicationDeliveryCommandsController { bus, drain }).controller()
}

#[derive(Debug, Clone)]
struct AnonymousApplicationDeliveryCommandsController {
    bus: Arc<CommandBus>,
    drain: DeliveryProcessDrain,
}

#[controller("/anonymous-delivery")]
#[metadata("auth.public", true)]
impl AnonymousApplicationDeliveryCommandsController {
    #[post(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions",
        raw
    )]
    async fn open_session(&self, request: BootRequest) -> Result<BootResponse> {
        self.drain.refuse_new_admission()?;
        let body: OpenAnonymousApplicationSessionRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(OpenAnonymousApplicationSession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                application_release_id: ApplicationReleaseId::from_uuid(body.release_id),
                session_id: ApplicationSessionId::from_uuid(body.session_id),
                credential_lookup_key: body.lookup_key,
                initial_variables: body.initial_variables,
                opened_at: Utc::now(),
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
        let body: RequestAnonymousApplicationInvocationRequest =
            request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        let response_mode =
            ApplicationResponseMode::parse(&body.response_mode).map_err(BootError::BadRequest)?;
        let ontology_digest =
            Sha256Digest::parse(body.ontology_digest).map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(RequestAnonymousApplicationInvocation {
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
                invocation_id: ApplicationInvocationId::from_uuid(body.invocation_id),
                expected_session_version: body.expected_session_version,
                credential_lookup_key: body.lookup_key,
                response_mode,
                input: body.input,
                ontology_id: OntologyId::from_uuid(body.ontology_id),
                ontology_revision_id: OntologyRevisionId::from_uuid(body.ontology_revision_id),
                ontology_digest,
                environment_id: body.environment_id.map(EnvironmentId::from_uuid),
                timeout_seconds: body.timeout_seconds.unwrap_or(
                    crate::modules::workflow::WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS,
                ),
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.invocation_replayed { 200 } else { 201 },
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
        let body: CloseAnonymousApplicationSessionRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CloseAnonymousApplicationSession {
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
                credential_lookup_key: body.lookup_key,
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
        let body: CancelAnonymousApplicationInvocationRequest =
            request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CancelAnonymousApplicationInvocation {
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
                credential_lookup_key: body.lookup_key,
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
mod nest_macro_anonymous_delivery_controller_tests {
    use super::*;

    #[test]
    fn anonymous_delivery_registers_public_routes_via_nest_macros() {
        let controller = anonymous_application_delivery_commands_controller(
            Arc::new(CommandBus::new()),
            DeliveryProcessDrain::default(),
        )
        .expect("anonymous delivery controller");
        assert_eq!(controller.prefix(), "/anonymous-delivery");
        assert_eq!(controller.routes().len(), 4);
        assert_eq!(
            controller.routes()[0].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions"
        );
        assert_eq!(
            controller.routes()[1].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations"
        );
        assert_eq!(
            controller.routes()[2].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/close"
        );
        assert_eq!(
            controller.routes()[3].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/cancel"
        );
    }
}
