use crate::modules::fleet::application::{GetNodePool, ListNodePools};
use crate::modules::fleet::presentation::dto::NodePoolResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{NodePoolId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_pool_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/node-pools",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListNodePools {
                            organization_id,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )?
        .get(
            "/{organization_id}/node-pools/{node_pool_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let node_pool_id =
                        NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetNodePool {
                            organization_id,
                            node_pool_id,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(pool) => {
                            BootResponse::json(&NodePoolResponse::new(pool, Utc::now(), false))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
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
