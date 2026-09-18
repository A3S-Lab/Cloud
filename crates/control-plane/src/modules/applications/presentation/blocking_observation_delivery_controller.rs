//! Applications management delivery for blocking invocation observation.
//!
//! `APP0.2-C41` exposes the C40 project-authorized CQRS poll through REST only.
//! No streaming cursor, SSE, Gateway route, or public availability is added.

use super::delivery_dto::ApplicationBlockingObservationResponse;
use crate::access_projection::application_access;
use crate::modules::applications::application::ObserveApplicationBlockingInvocation;
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

pub fn application_blocking_observation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationBlockingObservationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationBlockingObservationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationBlockingObservationQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation",
        raw
    )]
    async fn observe(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ObserveApplicationBlockingInvocation {
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
}

#[cfg(test)]
mod nest_macro_blocking_observation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn blocking_observation_registers_scoped_guarded_get_via_nest_macros() {
        let controller = application_blocking_observation_queries_controller(Arc::new(QueryBus::new()))
            .expect("blocking observation nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation"
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
