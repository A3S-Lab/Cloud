use crate::modules::projects::domain::entities::Project;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;

#[async_trait]
pub trait IProjectRepository: Send + Sync {
    async fn create(
        &self,
        project: Project,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Project>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Option<Project>, RepositoryError>;

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Project>, RepositoryError>;
}
