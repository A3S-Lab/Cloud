use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, OciArtifact, Workload, WorkloadRevision,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CreateDeploymentBundle {
    pub workload: Workload,
    pub revision: WorkloadRevision,
    pub deployment: Deployment,
    pub operation: OperationRequest,
    pub idempotency: crate::modules::shared_kernel::domain::IdempotencyRequest,
    pub event: a3s_cloud_contracts::DomainEventEnvelope,
}

#[derive(Clone)]
pub struct RequestDeploymentCancellationBundle {
    pub deployment: Deployment,
    pub expected_version: u64,
    pub idempotency: crate::modules::shared_kernel::domain::IdempotencyRequest,
    pub event: a3s_cloud_contracts::DomainEventEnvelope,
}

#[derive(Clone)]
pub struct RequestWorkloadStopBundle {
    pub workload: Workload,
    pub expected_version: u64,
    pub operation: OperationRequest,
    pub idempotency: IdempotencyRequest,
    pub event: a3s_cloud_contracts::DomainEventEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentBundle {
    pub workload: Workload,
    pub revision: WorkloadRevision,
    pub deployment: Deployment,
    pub operation: OperationRequest,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadStopBundle {
    pub workload: Workload,
    pub operation: OperationRequest,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRuntimeTarget {
    pub workload: Workload,
    pub revision: WorkloadRevision,
    pub deployment: Deployment,
}

#[async_trait]
pub trait IWorkloadRuntimeTargetRepository: Send + Sync {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadRepository: Send + Sync {
    async fn create_deployment(
        &self,
        bundle: CreateDeploymentBundle,
    ) -> Result<DeploymentBundle, RepositoryError>;

    async fn request_deployment_cancellation(
        &self,
        bundle: RequestDeploymentCancellationBundle,
    ) -> Result<crate::modules::shared_kernel::domain::IdempotentWrite<Deployment>, RepositoryError>;

    async fn replay_deployment_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<Deployment>, RepositoryError>;

    async fn request_workload_stop(
        &self,
        bundle: RequestWorkloadStopBundle,
    ) -> Result<WorkloadStopBundle, RepositoryError>;

    async fn complete_workload_stop(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        expected_version: u64,
        stopped_at: DateTime<Utc>,
    ) -> Result<Workload, RepositoryError>;

    async fn find_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Workload, RepositoryError>;

    async fn list_workloads(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Workload>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
    ) -> Result<WorkloadRevision, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadRevision>, RepositoryError>;

    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
        artifact: OciArtifact,
        resolved_at: DateTime<Utc>,
    ) -> Result<WorkloadRevision, RepositoryError>;

    async fn find_deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Deployment, RepositoryError>;

    async fn list_deployments(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<Deployment>, RepositoryError>;

    async fn mark_resolving(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn assign_node(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        node_id: NodeId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn mark_dispatched(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn mark_verifying(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn activate(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<(Workload, Deployment), RepositoryError>;

    async fn fail(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        reason: String,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn mark_cancellation_requested(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn begin_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn retry_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn cancel(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;
}
