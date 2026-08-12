use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodePoolId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetNodePool {
    pub organization_id: OrganizationId,
    pub node_pool_id: NodePoolId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetNodePool {
    type Output = ApplicationResult<NodePool>;
}

pub struct GetNodePoolHandler {
    node_pools: Arc<dyn INodePoolRepository>,
}

impl GetNodePoolHandler {
    pub fn new(node_pools: Arc<dyn INodePoolRepository>) -> Self {
        Self { node_pools }
    }
}

impl QueryHandler<GetNodePool> for GetNodePoolHandler {
    fn execute(
        &self,
        query: GetNodePool,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NodePool>>> {
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
            if !query.resource_access.is_organization_wide() {
                return Ok(Err(ApplicationError::Forbidden(
                    "node pool policy requires organization-wide access".into(),
                )));
            }
            match node_pools
                .find(query.organization_id, query.node_pool_id)
                .await
            {
                Ok(pool) => Ok(Ok(pool)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListNodePools {
    pub organization_id: OrganizationId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListNodePools {
    type Output = ApplicationResult<Vec<NodePool>>;
}

pub struct ListNodePoolsHandler {
    node_pools: Arc<dyn INodePoolRepository>,
}

impl ListNodePoolsHandler {
    pub fn new(node_pools: Arc<dyn INodePoolRepository>) -> Self {
        Self { node_pools }
    }
}

impl QueryHandler<ListNodePools> for ListNodePoolsHandler {
    fn execute(
        &self,
        query: ListNodePools,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<NodePool>>>> {
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
            if !query.resource_access.is_organization_wide() {
                return Ok(Err(ApplicationError::Forbidden(
                    "node pool policy requires organization-wide access".into(),
                )));
            }
            match node_pools.list(query.organization_id).await {
                Ok(pools) => Ok(Ok(pools)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
