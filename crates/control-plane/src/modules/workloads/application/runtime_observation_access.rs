use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_runtime::contract::{RuntimeHealthState, RuntimeUnitState};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Workloads-owned copy of the latest Runtime observation fields required by
/// deployment query enrichment. Fleet remains the observation authority; this
/// value contains only the consumer-facing snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRuntimeObservationProjection {
    pub report_id: Uuid,
    pub node_id: NodeId,
    pub command_id: Option<NodeCommandId>,
    pub unit_id: String,
    pub generation: u64,
    pub spec_digest: String,
    pub state: RuntimeUnitState,
    pub health_state: Option<RuntimeHealthState>,
    pub health_message: Option<String>,
    pub provider_resource_id: Option<String>,
    pub provider_build: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

/// Workloads-owned read port for deployment Runtime observation enrichment.
#[async_trait]
pub trait IWorkloadRuntimeObservationAccess: Send + Sync {
    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<WorkloadRuntimeObservationProjection>, RepositoryError>;
}
