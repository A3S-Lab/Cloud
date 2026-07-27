use super::arguments::{EmptyArguments, NodeArguments};
use super::tool_result;
use crate::modules::fleet::presentation::NodeResponse;
use crate::modules::fleet::{GetNode, ListNodes};
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::{QueryBus, Result};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn list_nodes(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListNodes {
            organization_id,
            queried_at: Utc::now(),
        })
        .await?
    {
        Ok(nodes) => tool_result::success(
            200,
            nodes
                .into_iter()
                .map(NodeResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_node(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: NodeArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetNode {
            organization_id,
            node_id: NodeId::from_uuid(arguments.node_id),
            queried_at: Utc::now(),
        })
        .await?
    {
        Ok(node) => tool_result::success(200, NodeResponse::from(node), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
