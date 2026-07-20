use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId,
    RepositoryError, WorkloadId, WorkloadRevisionId,
};
use a3s_runtime::contract::RuntimeUnitSpec;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRouteUpdateRequest {
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub previous_revision_id: WorkloadRevisionId,
    pub candidate_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub runtime_command_id: NodeCommandId,
    pub spec: RuntimeUnitSpec,
    pub verified_at: DateTime<Utc>,
    pub convergence_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentGatewayPublication {
    pub deployment_id: DeploymentId,
    pub node_id: NodeId,
    pub revision: u64,
    pub command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub command_not_after: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentRouteStage {
    NotRequired {
        checked_at: DateTime<Utc>,
    },
    Blocked {
        reason: String,
    },
    Staged {
        publication: DeploymentGatewayPublication,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentRouteObservation {
    Pending,
    Applied {
        acknowledged_at: DateTime<Utc>,
    },
    Rejected {
        reason: String,
        acknowledged_at: DateTime<Utc>,
    },
    Expired,
}

#[async_trait]
pub trait IDeploymentRouteUpdater: Send + Sync {
    async fn stage(
        &self,
        request: &DeploymentRouteUpdateRequest,
        now: DateTime<Utc>,
    ) -> Result<DeploymentRouteStage, RepositoryError>;

    async fn observe(
        &self,
        organization_id: OrganizationId,
        publication: &DeploymentGatewayPublication,
        now: DateTime<Utc>,
    ) -> Result<DeploymentRouteObservation, RepositoryError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnroutedDeploymentRouteUpdater;

#[async_trait]
impl IDeploymentRouteUpdater for UnroutedDeploymentRouteUpdater {
    async fn stage(
        &self,
        _request: &DeploymentRouteUpdateRequest,
        now: DateTime<Utc>,
    ) -> Result<DeploymentRouteStage, RepositoryError> {
        Ok(DeploymentRouteStage::NotRequired { checked_at: now })
    }

    async fn observe(
        &self,
        _organization_id: OrganizationId,
        _publication: &DeploymentGatewayPublication,
        _now: DateTime<Utc>,
    ) -> Result<DeploymentRouteObservation, RepositoryError> {
        Err(RepositoryError::Conflict(
            "unrouted deployment has no Gateway publication to observe".into(),
        ))
    }
}
