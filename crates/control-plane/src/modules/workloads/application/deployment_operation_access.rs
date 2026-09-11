use crate::modules::shared_kernel::domain::{OperationId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Workloads-owned copy of the immutable Operation projection fields required
/// by deployment query enrichment. Operations remains the status authority;
/// this value contains only the consumer-facing snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentOperationProjection {
    pub operation_id: OperationId,
    pub status: String,
    pub last_sequence: u64,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Workloads-owned read port for deployment Operation enrichment.
#[async_trait]
pub trait IWorkloadDeploymentOperationAccess: Send + Sync {
    async fn find_projection(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<WorkloadDeploymentOperationProjection>, RepositoryError>;
}
