use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError, SecretId, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, ManagedOwnerReference, OciArtifact, Workload,
    WorkloadControl, WorkloadControlSpec, WorkloadReplica, WorkloadReplicaMember, WorkloadRevision,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
pub struct CreateDeploymentBundle {
    pub workload: Workload,
    pub control: WorkloadControlSpec,
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

#[derive(Clone)]
pub struct ReconfigureReplicaSetWrite {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub expected_control_version: u64,
    pub expected_policy_generation: u64,
    pub desired_replicas: u32,
    pub managed_owner: Option<ManagedOwnerReference>,
    pub idempotency: IdempotencyRequest,
    pub correlation_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaSetWriteResult {
    pub control: WorkloadControl,
    pub replicas: Vec<WorkloadReplica>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaDeploymentCandidate {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_ordinal: u32,
    pub revision_id: WorkloadRevisionId,
    pub revision_generation: u64,
    pub replica_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaDeploymentMaterialization {
    pub candidate: ReplicaDeploymentCandidate,
    pub deployment: Deployment,
    pub operation: OperationRequest,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaEvacuationCandidate {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub expected_replica_version: u64,
    pub member_id: WorkloadReplicaMemberId,
    pub expected_member_version: u64,
    pub source_node_id: NodeId,
    pub placement_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaEvacuationRequest {
    pub candidate: ReplicaEvacuationCandidate,
    pub requested_at: DateTime<Utc>,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRuntimeTarget {
    pub workload: Workload,
    pub revision: WorkloadRevision,
    pub replica: WorkloadReplica,
    pub member: WorkloadReplicaMember,
    pub deployment: Deployment,
    pub replica_binding: DeploymentReplicaBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiringReplicaTarget {
    pub revision: WorkloadRevision,
    pub replica: WorkloadReplica,
    pub member: WorkloadReplicaMember,
    pub deployment: Option<Deployment>,
    pub replica_binding: Option<DeploymentReplicaBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaRetirementDispatch {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub expected_replica_version: u64,
    pub command_id: NodeCommandId,
    pub dispatched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaRuntimeFence {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub expected_replica_version: u64,
    pub command_id: NodeCommandId,
    pub fenced_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaRetirementCompletion {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub expected_replica_version: u64,
    pub member_id: WorkloadReplicaMemberId,
    pub expected_member_version: u64,
    pub fenced_node_id: Option<NodeId>,
    pub completed_at: DateTime<Utc>,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRotation {
    pub event_id: Uuid,
    pub correlation_id: Uuid,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub secret_id: SecretId,
    pub version: u64,
    pub occurred_at: DateTime<Utc>,
}

impl SecretRotation {
    pub fn validate(&self) -> Result<(), String> {
        if self.event_id.is_nil()
            || self.correlation_id.is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.version == 0
        {
            return Err("Secret rotation identity is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretRotationCompletion {
    Scheduled,
    NoTargets,
    Superseded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRotationReconciliation {
    pub scheduled: Vec<DeploymentBundle>,
    pub completion: Option<SecretRotationCompletion>,
}

#[async_trait]
pub trait ISecretRotationRestartRepository: Send + Sync {
    async fn pending_secret_rotations(
        &self,
        limit: usize,
    ) -> Result<Vec<SecretRotation>, RepositoryError>;

    async fn reconcile_secret_rotation(
        &self,
        rotation: SecretRotation,
        workload_limit: usize,
        reconciled_at: DateTime<Utc>,
    ) -> Result<SecretRotationReconciliation, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadRuntimeTargetRepository: Send + Sync {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadReplicaRetirementRepository: Send + Sync {
    async fn pending_replica_retirements(
        &self,
        limit: usize,
    ) -> Result<Vec<RetiringReplicaTarget>, RepositoryError>;

    async fn dispatch_replica_retirement(
        &self,
        dispatch: ReplicaRetirementDispatch,
    ) -> Result<WorkloadReplica, RepositoryError>;

    async fn record_replica_runtime_fenced(
        &self,
        fence: ReplicaRuntimeFence,
    ) -> Result<WorkloadReplica, RepositoryError>;

    async fn complete_replica_retirement(
        &self,
        completion: ReplicaRetirementCompletion,
    ) -> Result<IdempotentWrite<WorkloadReplica>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadReplicaEvacuationRepository: Send + Sync {
    async fn pending_replica_evacuations(
        &self,
        organization_id: OrganizationId,
        source_node_id: NodeId,
        limit: usize,
    ) -> Result<Vec<ReplicaEvacuationCandidate>, RepositoryError>;

    async fn request_replica_evacuation(
        &self,
        request: ReplicaEvacuationRequest,
    ) -> Result<IdempotentWrite<WorkloadReplica>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadReplicaDeploymentRepository: Send + Sync {
    async fn pending_replica_deployments(
        &self,
        limit: usize,
    ) -> Result<Vec<ReplicaDeploymentCandidate>, RepositoryError>;

    async fn materialize_replica_deployment(
        &self,
        candidate: ReplicaDeploymentCandidate,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<ReplicaDeploymentMaterialization>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadRepository: Send + Sync {
    async fn create_deployment(
        &self,
        bundle: CreateDeploymentBundle,
    ) -> Result<DeploymentBundle, RepositoryError>;

    async fn replay_deployment(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DeploymentBundle>, RepositoryError>;

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

    async fn reconfigure_replica_set(
        &self,
        write: ReconfigureReplicaSetWrite,
    ) -> Result<ReplicaSetWriteResult, RepositoryError>;

    async fn find_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Workload, RepositoryError>;

    async fn find_workload_control(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<WorkloadControl, RepositoryError>;

    async fn find_workload_replica(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        replica_id: WorkloadReplicaId,
    ) -> Result<WorkloadReplica, RepositoryError>;

    async fn list_workload_replicas(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadReplica>, RepositoryError>;

    async fn find_workload_replica_member(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        member_id: WorkloadReplicaMemberId,
    ) -> Result<WorkloadReplicaMember, RepositoryError>;

    async fn list_workload_replica_members(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
    ) -> Result<Vec<WorkloadReplicaMember>, RepositoryError>;

    async fn find_deployment_replica_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentReplicaBinding, RepositoryError>;

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
        retirement_required: bool,
        at: DateTime<Utc>,
    ) -> Result<(Workload, Deployment), RepositoryError>;

    async fn dispatch_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

    async fn complete_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError>;

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
