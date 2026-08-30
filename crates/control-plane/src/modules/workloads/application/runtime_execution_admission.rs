use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodePoolId, OrganizationId, ProjectId, RepositoryError,
    WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::WorkloadRuntimeExecutionBinding;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeploymentRuntimeExecutionAdmissionRequest {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    deployment_id: DeploymentId,
    node_pool_id: Option<NodePoolId>,
}

impl DeploymentRuntimeExecutionAdmissionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        node_pool_id: Option<NodePoolId>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            environment_id,
            workload_id,
            workload_revision_id,
            deployment_id,
            node_pool_id,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self
                .node_pool_id
                .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("Deployment Runtime execution admission request is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn deployment_id(&self) -> DeploymentId {
        self.deployment_id
    }

    pub const fn node_pool_id(&self) -> Option<NodePoolId> {
        self.node_pool_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedWorkloadRuntimeExecution {
    node_pool_id: NodePoolId,
    execution: WorkloadRuntimeExecutionBinding,
    authorized_at: DateTime<Utc>,
}

impl AdmittedWorkloadRuntimeExecution {
    pub fn new(
        node_pool_id: NodePoolId,
        execution: WorkloadRuntimeExecutionBinding,
        authorized_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        execution.validate()?;
        if node_pool_id.as_uuid().is_nil()
            || authorized_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(authorized_at)
        {
            return Err("admitted Workload Runtime execution is invalid".into());
        }
        Ok(Self {
            node_pool_id,
            execution,
            authorized_at,
        })
    }

    pub const fn node_pool_id(&self) -> NodePoolId {
        self.node_pool_id
    }

    pub const fn execution(&self) -> &WorkloadRuntimeExecutionBinding {
        &self.execution
    }

    pub fn into_execution(self) -> WorkloadRuntimeExecutionBinding {
        self.execution
    }

    pub const fn authorized_at(&self) -> DateTime<Utc> {
        self.authorized_at
    }
}

#[async_trait]
pub trait IWorkloadRuntimeExecutionAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: DeploymentRuntimeExecutionAdmissionRequest,
    ) -> Result<Option<AdmittedWorkloadRuntimeExecution>, RepositoryError>;
}

/// Default adapter for deployments whose logical Workload has no external
/// Runtime-identity policy owner.
pub struct NoWorkloadRuntimeExecutionAdmission;

#[async_trait]
impl IWorkloadRuntimeExecutionAdmissionPort for NoWorkloadRuntimeExecutionAdmission {
    async fn admit(
        &self,
        request: DeploymentRuntimeExecutionAdmissionRequest,
    ) -> Result<Option<AdmittedWorkloadRuntimeExecution>, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        Ok(None)
    }
}
