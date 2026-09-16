//! Authenticated `/delivery` observation polls (APP0.3-C6).
//!
//! Reuses Principal-bound ObserveApplication* CQRS under `application:invoke`
//! with exact Application ResourceGrant fail-closed. No SSE, Gateway, or embed.

use super::authenticated_delivery_controller::exact_application_access;
use super::delivery_dto::{
    ApplicationAsynchronousObservationResponse, ApplicationBlockingObservationResponse,
    ApplicationStreamingObservationResponse,
};
use crate::modules::applications::application::{
    ObserveApplicationAsynchronousInvocation, ObserveApplicationBlockingInvocation,
    ObserveApplicationStreamingInvocation,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    ApplicationInvocationId, ApplicationSessionId, OrganizationId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Authenticated published-application observation polls under `/delivery`.
pub fn authenticated_application_observation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(AuthenticatedApplicationObservationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct AuthenticatedApplicationObservationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/delivery")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_INVOKE])]
impl AuthenticatedApplicationObservationQueriesController {
    #[get(
        "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation",
        raw
    )]
    async fn blocking(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(ObserveApplicationBlockingInvocation {
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
                actor_principal_id: actor_principal_id(&request)?,
                access,
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
        let (access, project_id, application_id) = exact_application_access(&request)?;
        let after_sequence = request
            .optional_query_value_as::<u64>("afterSequence")?
            .unwrap_or_default();
        match self
            .bus
            .execute(ObserveApplicationStreamingInvocation {
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
                actor_principal_id: actor_principal_id(&request)?,
                access,
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
        let (access, project_id, application_id) = exact_application_access(&request)?;
        match self
            .bus
            .execute(ObserveApplicationAsynchronousInvocation {
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
                actor_principal_id: actor_principal_id(&request)?,
                access,
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
mod nest_macro_authenticated_observation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn authenticated_observation_registers_scoped_delivery_polls_via_nest_macros() {
        let controller = authenticated_application_observation_queries_controller(Arc::new(
            QueryBus::new(),
        ))
        .expect("authenticated observation nest controller");
        assert_eq!(controller.prefix(), "/delivery");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: Vec<_> = routes.iter().map(|route| route.path()).collect();
        assert!(paths.iter().any(|path| path.contains("blocking-observation")));
        assert!(paths.iter().any(|path| path.contains("streaming-observation")));
        assert!(paths
            .iter()
            .any(|path| path.contains("asynchronous-observation")));
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::APPLICATION_INVOKE])
        );
    }
}
