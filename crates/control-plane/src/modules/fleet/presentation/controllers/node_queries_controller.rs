use crate::access_projection::fleet_access;
use crate::modules::fleet::application::{GetNode, ListNodes};
use crate::modules::fleet::presentation::dto::NodeResponse;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, get, use_guard, BootError, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(NodeQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct NodeQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl NodeQueriesController {
    #[get("/{organization_id}/nodes", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        let access = fleet_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListNodes {
                organization_id,
                queried_at: Utc::now(),
                access,
            })
            .await?
        {
            Ok(nodes) => BootResponse::json(
                &nodes.into_iter().map(NodeResponse::from).collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/nodes/{node_id}", raw)]
    async fn get_one(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let node_id = NodeId::from_uuid(request.param_as::<Uuid>("node_id")?);
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetNode {
                organization_id,
                node_id,
                queried_at: Utc::now(),
            })
            .await?
        {
            Ok(node) => BootResponse::json(&NodeResponse::from(node)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

#[cfg(test)]
mod nest_macro_node_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn node_queries_register_guarded_gets_via_nest_macros() {
        let controller =
            node_queries_controller(Arc::new(QueryBus::new())).expect("node nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/nodes".to_string(),
                "/organizations/{organization_id}/nodes/{node_id}".to_string(),
            ])
        );
    }
}
