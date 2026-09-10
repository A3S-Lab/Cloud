use crate::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Workloads-owned deployment operation intent.
///
/// Operations remains authoritative for workflow request aggregates. Workloads
/// Application and Domain emit this intent; Infrastructure composes the foreign
/// aggregate at the persistence boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadDeploymentOperationIntent {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub revision_id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
    pub requested_at: DateTime<Utc>,
}

impl WorkloadDeploymentOperationIntent {
    pub fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
        revision_id: WorkloadRevisionId,
        workload_id: WorkloadId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            deployment_id,
            revision_id,
            workload_id,
            requested_at,
        }
    }
}

/// Workloads-owned stop operation intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadStopOperationIntent {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub requested_at: DateTime<Utc>,
}

impl WorkloadStopOperationIntent {
    pub fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            workload_id,
            requested_at,
        }
    }
}
