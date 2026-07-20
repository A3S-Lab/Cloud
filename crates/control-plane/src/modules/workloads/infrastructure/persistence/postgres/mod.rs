mod create;
mod queries;
mod rows;
mod secret_rotation_restarts;
mod stop;
mod transitions;

use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, OciArtifact, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    ISecretRotationRestartRepository, IWorkloadRepository, IWorkloadRuntimeTargetRepository,
    RequestDeploymentCancellationBundle, RequestWorkloadStopBundle, SecretRotation,
    SecretRotationReconciliation, WorkloadStopBundle,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

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

    async fn find_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Workload, RepositoryError> {
        queries::find_workload(&self.executor, organization_id, workload_id).await
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
impl IWorkloadRuntimeTargetRepository for PostgresWorkloadRepository {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError> {
        queries::list_active_runtime_targets(&self.executor, limit).await
    }
}
