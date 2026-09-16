use crate::access_projection::fleet_access;
use crate::modules::fleet::application::{GetNodePool, ListNodePools};
use crate::modules::fleet::presentation::dto::NodePoolResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::domain::{NodePoolId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_pool_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(NodePoolQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct NodePoolQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl NodePoolQueriesController {
    #[get("/{organization_id}/node-pools", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        let access = fleet_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListNodePools {
                organization_id,
                access,
            })
            .await?
        {
            Ok(pools) => {
                let evaluated_at = Utc::now();
                BootResponse::json(
                    &pools
                        .into_iter()
                        .map(|pool| NodePoolResponse::new(pool, evaluated_at, false))
                        .collect::<Vec<_>>(),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/node-pools/{node_pool_id}", raw)]
    async fn get_one(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let node_pool_id = NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
        let request_id = request_id(&request)?;
        let access = fleet_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(GetNodePool {
                organization_id,
                node_pool_id,
                access,
            })
            .await?
        {
            Ok(pool) => BootResponse::json(&NodePoolResponse::new(pool, Utc::now(), false)),
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
mod nest_macro_node_pool_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn node_pool_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller = node_pool_queries_controller(Arc::new(QueryBus::new()))
            .expect("node pool nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/node-pools".to_string(),
                "/organizations/{organization_id}/node-pools/{node_pool_id}".to_string(),
            ])
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }
}
