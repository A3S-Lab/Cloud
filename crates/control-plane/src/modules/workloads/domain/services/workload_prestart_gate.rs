use crate::modules::shared_kernel::domain::{
    DeploymentId, NodeId, OperationId, OrganizationId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPrestartGateRequest {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub cancellation_requested: bool,
    pub deadline_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadPrestartGateStatus {
    Ready { completed_at: DateTime<Utc> },
    Pending { reason: String },
    Failed { reason: String },
    CancellationReady { completed_at: DateTime<Utc> },
}

/// Optional owner-specific work that must finish on the selected node before
/// Workloads dispatches its Service. The Workload Flow remains the sole
/// deployment lifecycle and polling authority.
#[async_trait]
pub trait IWorkloadPrestartGate: Send + Sync {
    async fn reconcile(
        &self,
        request: &WorkloadPrestartGateRequest,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnrestrictedWorkloadPrestartGate;

#[async_trait]
impl IWorkloadPrestartGate for UnrestrictedWorkloadPrestartGate {
    async fn reconcile(
        &self,
        request: &WorkloadPrestartGateRequest,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        Ok(if request.cancellation_requested {
            WorkloadPrestartGateStatus::CancellationReady {
                completed_at: request.now,
            }
        } else {
            WorkloadPrestartGateStatus::Ready {
                completed_at: request.now,
            }
        })
    }
}
