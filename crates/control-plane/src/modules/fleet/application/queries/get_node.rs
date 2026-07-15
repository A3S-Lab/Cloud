use super::NodeQueryResult;
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetNode {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub queried_at: DateTime<Utc>,
}

impl Query for GetNode {
    type Output = ApplicationResult<NodeQueryResult>;
}

pub struct GetNodeHandler {
    nodes: Arc<dyn INodeRepository>,
    heartbeat_timeout: Duration,
}

impl GetNodeHandler {
    pub fn new(
        nodes: Arc<dyn INodeRepository>,
        heartbeat_timeout: Duration,
    ) -> Result<Self, String> {
        if heartbeat_timeout <= Duration::zero() {
            return Err("node heartbeat timeout must be positive".into());
        }
        Ok(Self {
            nodes,
            heartbeat_timeout,
        })
    }
}

impl QueryHandler<GetNode> for GetNodeHandler {
    fn execute(
        &self,
        query: GetNode,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NodeQueryResult>>> {
        let nodes = Arc::clone(&self.nodes);
        let heartbeat_timeout = self.heartbeat_timeout;
        Box::pin(async move {
            match nodes.find(query.organization_id, query.node_id).await {
                Ok(node) => {
                    let availability = node.availability_at(query.queried_at, heartbeat_timeout);
                    Ok(Ok(NodeQueryResult { node, availability }))
                }
                Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    Ok(Err(ApplicationError::NotFound("node not found".into())))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
