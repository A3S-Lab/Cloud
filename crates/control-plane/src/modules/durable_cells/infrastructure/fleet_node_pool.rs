use crate::modules::durable_cells::application::{
    DurableCellNodePoolSelectionRequest, IDurableCellNodePoolPort,
};
use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from Fleet's node-pool repository to the
/// consumer-owned Durable Cells placement port. Fleet remains responsible for
/// pool lifecycle, capacity, claims, and scheduling.
#[derive(Clone)]
pub struct FleetDurableCellNodePoolAdapter {
    node_pools: Arc<dyn INodePoolRepository>,
}

impl FleetDurableCellNodePoolAdapter {
    pub fn new(node_pools: Arc<dyn INodePoolRepository>) -> Self {
        Self { node_pools }
    }
}

#[async_trait]
impl IDurableCellNodePoolPort for FleetDurableCellNodePoolAdapter {
    async fn validate_selection(
        &self,
        request: &DurableCellNodePoolSelectionRequest,
    ) -> ApplicationResult<()> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let Some(node_pool_id) = request.node_pool_id else {
            return Ok(());
        };
        match self
            .node_pools
            .find(request.organization_id, node_pool_id)
            .await
        {
            Ok(pool)
                if pool.organization_id == request.organization_id && pool.id == node_pool_id =>
            {
                Ok(())
            }
            Ok(_) | Err(RepositoryError::NotFound) => {
                Err(ApplicationError::NotFound("node pool not found".into()))
            }
            Err(error) => Err(error.into()),
        }
    }
}
