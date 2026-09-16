//! Public anonymous delivery for Applications observation polls.
//!
//! `APP0.2-C54` exposes C48/C49/C50 CQRS over `/anonymous-delivery` with
//! opaque `lookupKey` query auth. No SSE, Gateway, or project-member scope.

use super::delivery_dto::{
    ApplicationAsynchronousObservationResponse, ApplicationBlockingObservationResponse,
    ApplicationStreamingObservationResponse,
};
use crate::modules::applications::application::{
    ObserveAnonymousApplicationAsynchronousInvocation,
    ObserveAnonymousApplicationBlockingInvocation, ObserveAnonymousApplicationStreamingInvocation,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, ProjectId,
};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, BootError, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result, AUTH_PUBLIC_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

fn required_lookup_key(request: &BootRequest) -> Result<String> {
    request
        .query_value("lookupKey")?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("lookupKey query parameter is required".into()))
}

/// Public anonymous observation polls over opaque credential lookup keys.
pub fn anonymous_application_observation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(AnonymousApplicationObservationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct AnonymousApplicationObservationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/anonymous-delivery")]
#[metadata("auth.public", true)]
impl AnonymousApplicationObservationQueriesController {
    #[get(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation",
        raw
    )]
    async fn blocking(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let credential_lookup_key = required_lookup_key(&request)?;
        match self
            .bus
            .execute(ObserveAnonymousApplicationBlockingInvocation {
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
                credential_lookup_key,
                observed_at: Utc::now(),
            })
            .await?
        {
            Ok(observation) => {
                BootResponse::json(&ApplicationBlockingObservationResponse::from(observation))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/streaming-observation",
        raw
    )]
    async fn streaming(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let credential_lookup_key = required_lookup_key(&request)?;
        let after_sequence = request
            .optional_query_value_as::<u64>("afterSequence")?
            .unwrap_or_default();
        match self
            .bus
            .execute(ObserveAnonymousApplicationStreamingInvocation {
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
                credential_lookup_key,
                after_sequence,
                observed_at: Utc::now(),
            })
            .await?
        {
            Ok(observation) => {
                BootResponse::json(&ApplicationStreamingObservationResponse::from(observation))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/asynchronous-observation",
        raw
    )]
    async fn asynchronous(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let credential_lookup_key = required_lookup_key(&request)?;
        match self
            .bus
            .execute(ObserveAnonymousApplicationAsynchronousInvocation {
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
                credential_lookup_key,
                observed_at: Utc::now(),
            })
            .await?
        {
            Ok(observation) => BootResponse::json(
                &ApplicationAsynchronousObservationResponse::from(observation),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_anonymous_observation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn anonymous_observation_registers_public_polls_via_nest_macros() {
        let controller =
            anonymous_application_observation_queries_controller(Arc::new(QueryBus::new()))
                .expect("anonymous observation nest controller");
        assert_eq!(controller.prefix(), "/anonymous-delivery");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        assert_eq!(
            routes[0].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation"
        );
        assert_eq!(
            routes[1].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/streaming-observation"
        );
        assert_eq!(
            routes[2].path(),
            "/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/asynchronous-observation"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_PUBLIC_METADATA)
                .cloned()
                .expect("auth.public"),
            serde_json::json!(true)
        );
    }
}
