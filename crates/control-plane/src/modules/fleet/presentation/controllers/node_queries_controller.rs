use crate::modules::fleet::application::{GetNode, ListNodes};
use crate::modules::fleet::presentation::dto::NodeResponse;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get("/{organization_id}/nodes", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListNodes {
                        organization_id,
                        queried_at: Utc::now(),
                    })
                    .await?
                {
                    Ok(nodes) => BootResponse::json(
                        &nodes
                            .into_iter()
                            .map(NodeResponse::from)
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .get(
            "/{organization_id}/nodes/{node_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let node_id = NodeId::from_uuid(request.param_as::<Uuid>("node_id")?);
                    let request_id = request_id(&request)?;
                    match bus
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
