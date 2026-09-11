use crate::modules::edge::application::{
    EdgeRuntimeObservationProjection, IEdgeRuntimeObservationAccess,
};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for Fleet Runtime observation cutover reads.
#[derive(Clone)]
pub struct FleetEdgeRuntimeObservationAccessAdapter {
    node_control: Arc<dyn INodeControlRepository>,
}

impl FleetEdgeRuntimeObservationAccessAdapter {
    pub fn new(node_control: Arc<dyn INodeControlRepository>) -> Self {
        Self { node_control }
    }
}

#[async_trait]
impl IEdgeRuntimeObservationAccess for FleetEdgeRuntimeObservationAccessAdapter {
    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<EdgeRuntimeObservationProjection>, RepositoryError> {
        Ok(self
            .node_control
            .latest_runtime_observation(node_id, unit_id, generation)
            .await?
            .map(|record| EdgeRuntimeObservationProjection {
                command_id: record.command_id,
                received_at: record.received_at,
                observation: record.observation,
            }))
    }
}
