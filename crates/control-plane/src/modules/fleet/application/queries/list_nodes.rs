use super::NodeQueryResult;
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListNodes {
    pub organization_id: OrganizationId,
    pub queried_at: DateTime<Utc>,
}

impl Query for ListNodes {
    type Output = ApplicationResult<Vec<NodeQueryResult>>;
}

pub struct ListNodesHandler {
    nodes: Arc<dyn INodeRepository>,
    heartbeat_timeout: Duration,
}

impl ListNodesHandler {
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

impl QueryHandler<ListNodes> for ListNodesHandler {
    fn execute(
        &self,
        query: ListNodes,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<NodeQueryResult>>>>
    {
        let nodes = Arc::clone(&self.nodes);
        let heartbeat_timeout = self.heartbeat_timeout;
        Box::pin(async move {
            match nodes.list(query.organization_id).await {
                Ok(nodes) => Ok(Ok(nodes
                    .into_iter()
                    .map(|node| NodeQueryResult {
                        availability: node.availability_at(query.queried_at, heartbeat_timeout),
                        node,
                    })
                    .collect())),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
