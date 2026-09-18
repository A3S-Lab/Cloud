//! Applications management delivery for streaming invocation observation.
//!
//! `APP0.2-C44` exposes the C43 project-authorized CQRS poll through REST only.
//! Optional `afterSequence` cursor defaults to zero. No SSE, Gateway route, or
//! public availability is added.

use super::delivery_dto::ApplicationStreamingObservationResponse;
use crate::access_projection::application_access;
use crate::modules::applications::application::ObserveApplicationStreamingInvocation;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, ProjectId,
};
use crate::presentation::{
    OrganizationTenantGuard, resource_access_evaluator, actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, use_guard, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result, AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_streaming_observation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationStreamingObservationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationStreamingObservationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationStreamingObservationQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/streaming-observation",
        raw
    )]
    async fn observe(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let after_sequence = request
            .optional_query_value_as::<u64>("afterSequence")?
            .unwrap_or_default();
        match self
            .bus
            .execute(ObserveApplicationStreamingInvocation {
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
}

#[cfg(test)]
mod nest_macro_streaming_observation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn streaming_observation_registers_scoped_guarded_get_via_nest_macros() {
        let controller =
            application_streaming_observation_queries_controller(Arc::new(QueryBus::new()))
                .expect("streaming observation nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/streaming-observation"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::APPLICATION_WRITE])
        );
    }
}
