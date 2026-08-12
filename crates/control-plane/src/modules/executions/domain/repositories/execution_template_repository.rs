use crate::modules::executions::domain::ExecutionTemplateRevision;
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Clone)]
pub struct CreateExecutionTemplateRevision {
    pub revision: ExecutionTemplateRevision,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IExecutionTemplateRepository: Send + Sync {
    async fn replay_create(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ExecutionTemplateRevision>>, RepositoryError>;

    async fn create(
        &self,
        write: CreateExecutionTemplateRevision,
    ) -> Result<IdempotentWrite<ExecutionTemplateRevision>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        template_id: ExecutionTemplateId,
        revision_id: ExecutionTemplateRevisionId,
    ) -> Result<Option<ExecutionTemplateRevision>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<ExecutionTemplateRevision>, RepositoryError>;
}
