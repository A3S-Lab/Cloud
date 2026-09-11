use crate::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

/// Workloads-owned writer-fence continuation intent.
///
/// Owner adapters emit this handoff with the exact subject, workflow, and
/// opaque input required for the continuation. Infrastructure composes the
/// foreign Operations aggregate when persisting the Runtime fence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadWriterFenceContinuationIntent {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub subject_kind: String,
    pub subject_id: Uuid,
    pub workflow_name: String,
    pub workflow_version: String,
    pub input: serde_json::Value,
    pub requested_at: DateTime<Utc>,
}

impl WorkloadWriterFenceContinuationIntent {
    pub fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        subject_kind: impl Into<String>,
        subject_id: Uuid,
        workflow_name: impl Into<String>,
        workflow_version: impl Into<String>,
        input: serde_json::Value,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            subject_kind: subject_kind.into(),
            subject_id,
            workflow_name: workflow_name.into(),
            workflow_version: workflow_version.into(),
            input,
            requested_at,
        }
    }
}
