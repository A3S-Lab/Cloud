use crate::modules::edge::application::PublishRoute;
use crate::modules::edge::presentation::dto::{PublishRouteRequest, RoutePublicationResponse};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, OrganizationId, ProjectId, WorkloadRevisionId,
};
use crate::presentation::{OrganizationTenantGuard, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_identity;

pub fn routes_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(RoutesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct RoutesController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::ROUTE_WRITE])]
impl RoutesController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/routes",
        raw
    )]
    async fn publish(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PublishRouteRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(PublishRoute {
                organization_id,
                project_id,
                environment_id,
                gateway_scope_id: GatewayScopeId::from_uuid(body.gateway_scope_id),
                workload_revision_id: WorkloadRevisionId::from_uuid(body.workload_revision_id),
                domain_claim_id: DomainClaimId::from_uuid(body.domain_claim_id),
                hostname: body.hostname,
                path_prefix: body.path_prefix,
                port_name: body.port_name,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.publication.replayed { 200 } else { 202 };
                BootResponse::json_with_status(status, &RoutePublicationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_routes_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn routes_controller_registers_scoped_guarded_post_via_nest_macros() {
        let controller =
            routes_controller(Arc::new(CommandBus::new())).expect("routes nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/routes"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::ROUTE_WRITE])
        );
    }
}
