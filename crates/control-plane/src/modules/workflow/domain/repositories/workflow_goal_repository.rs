use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    RepositoryError, WorkflowGoalId,
};
use crate::modules::workflow::domain::{PlanRevision, WorkflowGoal};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGoalRecord {
    pub goal: WorkflowGoal,
    pub plan_revision: PlanRevision,
}

#[derive(Debug, Clone)]
pub struct CreateWorkflowGoalWrite {
    pub record: WorkflowGoalRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkflowGoalWriteReference {
    pub organization_id: OrganizationId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
}

#[async_trait]
pub trait IWorkflowGoalRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateWorkflowGoalWrite,
    ) -> Result<IdempotentWrite<WorkflowGoalRecord>, RepositoryError>;

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowGoalRecord>, RepositoryError>;

    async fn find_plan_revision(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
        plan_revision_id: PlanRevisionId,
    ) -> Result<Option<PlanRevision>, RepositoryError>;
}
