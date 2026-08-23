use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileNodeAvailability {
    pub evaluated_at: DateTime<Utc>,
    pub heartbeat_timeout: Duration,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeAvailabilityReconciliationResult {
    pub processed_nodes: usize,
    pub initialized_heads: usize,
    pub unavailable_facts: usize,
}

#[async_trait]
pub trait INodeAvailabilityRepository: Send + Sync {
    async fn reconcile_node_availability(
        &self,
        request: ReconcileNodeAvailability,
    ) -> Result<NodeAvailabilityReconciliationResult, RepositoryError>;
}
