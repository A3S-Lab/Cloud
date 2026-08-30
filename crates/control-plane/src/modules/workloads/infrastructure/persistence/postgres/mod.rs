mod create;
mod deployment_group_bindings;
mod operation_requests;
mod placement_group_scheduling;
mod placement_groups;
mod queries;
mod replica_deployment_materialization;
mod replica_evacuations;
mod replica_retirements;
mod replica_set_reconfiguration;
mod replicas;
mod resource_claim_rows;
mod resource_claim_writes;
mod resource_claims;
mod rows;
mod runtime_execution_bindings;
mod schema;
mod secret_rotation_restarts;
mod stop;
mod transitions;
mod writer_fences;

use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadPlacementGroupId, WorkloadReplicaId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentPlacementGroupBinding, DeploymentReplicaBinding,
    DeploymentRuntimeExecutionBinding, OciArtifact, Workload, WorkloadControl, WorkloadReplica,
    WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    ISecretRotationRestartRepository, IWorkloadPlacementGroupRepository,
    IWorkloadPlacementGroupSchedulingRepository, IWorkloadReplicaDeploymentRepository,
    IWorkloadReplicaEvacuationRepository, IWorkloadReplicaRetirementRepository,
    IWorkloadRepository, IWorkloadRuntimeTargetRepository, IWorkloadWriterFenceRepository,
    PlacementGroupCancellationWrite, PlacementGroupMaterialization, PlacementGroupPlacement,
    PlacementGroupSchedulingWrite, ReconfigureReplicaSetWrite, ReplicaDeploymentCandidate,
    ReplicaDeploymentMaterialization, ReplicaEvacuationCandidate, ReplicaEvacuationRequest,
    ReplicaRetirementCompletion, ReplicaRetirementDispatch, ReplicaRuntimeFence,
    ReplicaSetWriteResult, RequestDeploymentCancellationBundle, RequestWorkloadStopBundle,
    RetiringReplicaTarget, SecretRotation, SecretRotationReconciliation, WorkloadStopBundle,
    WorkloadWriterFenceCommit,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub use resource_claims::PostgresResourceClaimRepository;

#[derive(Clone)]
pub struct PostgresWorkloadRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkloadRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IWorkloadPlacementGroupRepository for PostgresWorkloadRepository {
    async fn materialize_placement_group(
        &self,
        write: crate::modules::workloads::domain::entities::WorkloadPlacementGroupWrite,
    ) -> Result<PlacementGroupMaterialization, RepositoryError> {
        placement_groups::materialize(&self.executor, write).await
    }

    async fn find_placement_group(
        &self,
        organization_id: OrganizationId,
        group_id: WorkloadPlacementGroupId,
    ) -> Result<crate::modules::workloads::domain::entities::WorkloadPlacementGroup, RepositoryError>
    {
        placement_groups::find(&self.executor, organization_id, group_id).await
    }

    async fn find_placement_group_for_replica_generation(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
    ) -> Result<crate::modules::workloads::domain::entities::WorkloadPlacementGroup, RepositoryError>
    {
        placement_groups::find_for_replica_generation(
            &self.executor,
            organization_id,
            replica_id,
            replica_generation,
        )
        .await
    }
}

#[async_trait]
impl IWorkloadPlacementGroupSchedulingRepository for PostgresWorkloadRepository {
    async fn schedule_placement_group(
        &self,
        write: PlacementGroupSchedulingWrite,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<PlacementGroupPlacement>,
        RepositoryError,
    > {
        placement_group_scheduling::schedule(&self.executor, write).await
    }

    async fn cancel_placement_group(
        &self,
        write: PlacementGroupCancellationWrite,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<PlacementGroupPlacement>,
        RepositoryError,
    > {
        placement_group_scheduling::cancel(&self.executor, write).await
    }
}

#[async_trait]
impl IWorkloadRepository for PostgresWorkloadRepository {
    async fn create_deployment(
        &self,
        bundle: CreateDeploymentBundle,
    ) -> Result<DeploymentBundle, RepositoryError> {
        create::deployment(&self.executor, bundle).await
    }

    async fn replay_deployment(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DeploymentBundle>, RepositoryError> {
        create::replay(&self.executor, idempotency).await
    }

    async fn request_deployment_cancellation(
        &self,
        bundle: RequestDeploymentCancellationBundle,
    ) -> Result<crate::modules::shared_kernel::domain::IdempotentWrite<Deployment>, RepositoryError>
    {
        transitions::request_cancellation(&self.executor, bundle).await
    }

    async fn replay_deployment_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<Deployment>, RepositoryError> {
        transitions::cancellation_replay(&self.executor, idempotency).await
    }

    async fn request_workload_stop(
        &self,
        bundle: RequestWorkloadStopBundle,
    ) -> Result<WorkloadStopBundle, RepositoryError> {
        stop::request(&self.executor, bundle).await
    }

    async fn complete_workload_stop(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        expected_version: u64,
        stopped_at: DateTime<Utc>,
    ) -> Result<Workload, RepositoryError> {
        stop::complete(
            &self.executor,
            organization_id,
            workload_id,
            expected_version,
            stopped_at,
        )
        .await
    }

    async fn reconfigure_replica_set(
        &self,
        write: ReconfigureReplicaSetWrite,
    ) -> Result<ReplicaSetWriteResult, RepositoryError> {
        replica_set_reconfiguration::reconfigure(&self.executor, write).await
    }

    async fn find_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Workload, RepositoryError> {
        queries::find_workload(&self.executor, organization_id, workload_id).await
    }

    async fn find_workload_control(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<WorkloadControl, RepositoryError> {
        replicas::find_control(&self.executor, organization_id, workload_id).await
    }

    async fn find_workload_replica(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        replica_id: WorkloadReplicaId,
    ) -> Result<WorkloadReplica, RepositoryError> {
        replicas::find_replica(&self.executor, organization_id, workload_id, replica_id).await
    }

    async fn list_workload_replicas(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadReplica>, RepositoryError> {
        replicas::list_replicas(&self.executor, organization_id, workload_id).await
    }

    async fn find_workload_replica_member(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        member_id: WorkloadReplicaMemberId,
    ) -> Result<WorkloadReplicaMember, RepositoryError> {
        replicas::find_member(&self.executor, organization_id, replica_id, member_id).await
    }

    async fn list_workload_replica_members(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
    ) -> Result<Vec<WorkloadReplicaMember>, RepositoryError> {
        replicas::list_members(&self.executor, organization_id, replica_id).await
    }

    async fn find_deployment_replica_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentReplicaBinding, RepositoryError> {
        replicas::find_binding(&self.executor, organization_id, deployment_id).await
    }

    async fn list_deployment_replica_member_bindings(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Vec<DeploymentReplicaBinding>, RepositoryError> {
        deployment_group_bindings::list_member_bindings(
            &self.executor,
            organization_id,
            deployment_id,
        )
        .await
    }

    async fn find_deployment_placement_group_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentPlacementGroupBinding, RepositoryError> {
        deployment_group_bindings::find_group_binding(
            &self.executor,
            organization_id,
            deployment_id,
        )
        .await
    }

    async fn bind_deployment_runtime_execution(
        &self,
        binding: DeploymentRuntimeExecutionBinding,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<DeploymentRuntimeExecutionBinding>,
        RepositoryError,
    > {
        runtime_execution_bindings::bind(&self.executor, binding).await
    }

    async fn find_deployment_runtime_execution_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Option<DeploymentRuntimeExecutionBinding>, RepositoryError> {
        runtime_execution_bindings::find(&self.executor, organization_id, deployment_id).await
    }

    async fn list_workloads(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Workload>, RepositoryError> {
        queries::list_workloads(&self.executor, organization_id, project_id, environment_id).await
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
    ) -> Result<WorkloadRevision, RepositoryError> {
        queries::find_revision(&self.executor, organization_id, revision_id).await
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadRevision>, RepositoryError> {
        queries::list_revisions(&self.executor, organization_id, workload_id).await
    }

    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
        artifact: OciArtifact,
        resolved_at: DateTime<Utc>,
    ) -> Result<WorkloadRevision, RepositoryError> {
        transitions::resolve_revision(
            &self.executor,
            organization_id,
            revision_id,
            artifact,
            resolved_at,
        )
        .await
    }

    async fn find_deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Deployment, RepositoryError> {
        queries::find_deployment(&self.executor, organization_id, deployment_id).await
    }

    async fn list_deployments(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<Deployment>, RepositoryError> {
        queries::list_deployments(&self.executor, organization_id, workload_id).await
    }

    async fn mark_resolving(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Resolve { at },
        )
        .await
    }

    async fn assign_node(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        node_id: NodeId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Schedule { node_id, at },
        )
        .await
    }

    async fn mark_dispatched(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Dispatch { command_id, at },
        )
        .await
    }

    async fn mark_verifying(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Verify { at },
        )
        .await
    }

    async fn activate(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        retirement_required: bool,
        at: DateTime<Utc>,
    ) -> Result<(Workload, Deployment), RepositoryError> {
        transitions::activate(
            &self.executor,
            deployment_id,
            expected_version,
            retirement_required,
            at,
        )
        .await
    }

    async fn dispatch_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::DispatchRetirement { command_id, at },
        )
        .await
    }

    async fn complete_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::CompleteRetirement { at },
        )
        .await
    }

    async fn fail(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        reason: String,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Fail { reason, at },
        )
        .await
    }

    async fn mark_cancellation_requested(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::RequestCancellation { at },
        )
        .await
    }

    async fn begin_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::BeginCleanup { command_id, at },
        )
        .await
    }

    async fn retry_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::RetryCleanup { command_id, at },
        )
        .await
    }

    async fn cancel(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        transitions::mutate(
            &self.executor,
            deployment_id,
            expected_version,
            transitions::DeploymentMutation::Cancel { at },
        )
        .await
    }
}

#[async_trait]
impl ISecretRotationRestartRepository for PostgresWorkloadRepository {
    async fn pending_secret_rotations(
        &self,
        limit: usize,
    ) -> Result<Vec<SecretRotation>, RepositoryError> {
        secret_rotation_restarts::pending(&self.executor, limit).await
    }

    async fn reconcile_secret_rotation(
        &self,
        rotation: SecretRotation,
        workload_limit: usize,
        reconciled_at: DateTime<Utc>,
    ) -> Result<SecretRotationReconciliation, RepositoryError> {
        secret_rotation_restarts::reconcile(&self.executor, rotation, workload_limit, reconciled_at)
            .await
    }
}

#[async_trait]
impl IWorkloadReplicaDeploymentRepository for PostgresWorkloadRepository {
    async fn pending_replica_deployments(
        &self,
        limit: usize,
    ) -> Result<Vec<ReplicaDeploymentCandidate>, RepositoryError> {
        replica_deployment_materialization::pending(&self.executor, limit).await
    }

    async fn materialize_replica_deployment(
        &self,
        candidate: ReplicaDeploymentCandidate,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<ReplicaDeploymentMaterialization>, RepositoryError> {
        replica_deployment_materialization::materialize(&self.executor, candidate, requested_at)
            .await
    }
}

#[async_trait]
impl IWorkloadReplicaEvacuationRepository for PostgresWorkloadRepository {
    async fn has_replica_placements(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<bool, RepositoryError> {
        replica_evacuations::has_placements(&self.executor, organization_id, node_id).await
    }

    async fn pending_replica_evacuations(
        &self,
        organization_id: OrganizationId,
        source_node_id: NodeId,
        limit: usize,
    ) -> Result<Vec<ReplicaEvacuationCandidate>, RepositoryError> {
        replica_evacuations::pending(&self.executor, organization_id, source_node_id, limit).await
    }

    async fn request_replica_evacuation(
        &self,
        request: ReplicaEvacuationRequest,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<WorkloadReplica>,
        RepositoryError,
    > {
        replica_evacuations::request(&self.executor, request).await
    }
}

#[async_trait]
impl IWorkloadReplicaRetirementRepository for PostgresWorkloadRepository {
    async fn pending_replica_retirements(
        &self,
        limit: usize,
    ) -> Result<Vec<RetiringReplicaTarget>, RepositoryError> {
        replica_retirements::pending(&self.executor, limit).await
    }

    async fn dispatch_replica_retirement(
        &self,
        dispatch: ReplicaRetirementDispatch,
    ) -> Result<WorkloadReplica, RepositoryError> {
        replica_retirements::dispatch(&self.executor, dispatch).await
    }

    async fn record_replica_runtime_fenced(
        &self,
        fence: ReplicaRuntimeFence,
        writer_fence: Option<WorkloadWriterFenceCommit>,
    ) -> Result<WorkloadReplica, RepositoryError> {
        replica_retirements::record_fence(&self.executor, fence, writer_fence).await
    }

    async fn complete_replica_retirement(
        &self,
        completion: ReplicaRetirementCompletion,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<WorkloadReplica>,
        RepositoryError,
    > {
        replica_retirements::complete(&self.executor, completion).await
    }
}

#[async_trait]
impl IWorkloadWriterFenceRepository for PostgresWorkloadRepository {
    async fn latest_writer_fence(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Option<crate::modules::workloads::WorkloadWriterFenceReceipt>, RepositoryError>
    {
        writer_fences::latest(&self.executor, organization_id, workload_id).await
    }
}

#[async_trait]
impl IWorkloadRuntimeTargetRepository for PostgresWorkloadRepository {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError> {
        queries::list_active_runtime_targets(&self.executor, limit).await
    }
}
