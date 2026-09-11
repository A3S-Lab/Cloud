use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_runtime::contract::RuntimeObservation;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Edge-owned copy of the latest Runtime observation fields required by
/// Gateway route cutover. Fleet remains the observation authority; this value
/// contains only the consumer-facing snapshot plus published Runtime contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRuntimeObservationProjection {
    pub command_id: Option<NodeCommandId>,
    pub received_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

/// Edge-owned read port for candidate route-target Runtime observation.
#[async_trait]
pub trait IEdgeRuntimeObservationAccess: Send + Sync {
    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<EdgeRuntimeObservationProjection>, RepositoryError>;
}
