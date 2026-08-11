use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    WorkflowRunId,
};
use crate::modules::workflow::domain::WorkflowRun;
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StartWorkflowRunWrite {
    pub run: WorkflowRun,
    pub operation: OperationRequest,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkflowRunWriteReference {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
}

#[async_trait]
pub trait IWorkflowRunRepository: Send + Sync {
    async fn start(
        &self,
        write: StartWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRun>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<WorkflowRun>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowRun>, RepositoryError>;
}
