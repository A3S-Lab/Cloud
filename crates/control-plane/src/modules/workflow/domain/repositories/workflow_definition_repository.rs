use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{WorkflowDefinition, WorkflowRevision};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionRecord {
    pub definition: WorkflowDefinition,
    pub revision: WorkflowRevision,
}

#[derive(Debug, Clone)]
pub struct CreateWorkflowDefinitionWrite {
    pub record: WorkflowDefinitionRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ReviseWorkflowDefinitionWrite {
    pub record: WorkflowDefinitionRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkflowDefinitionWriteReference {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
}

#[async_trait]
pub trait IWorkflowDefinitionRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError>;

    async fn revise(
        &self,
        write: ReviseWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError>;

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowDefinitionRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Option<WorkflowDefinition>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowDefinition>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
        revision_id: WorkflowRevisionId,
    ) -> Result<Option<WorkflowRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Vec<WorkflowRevision>, RepositoryError>;
}
