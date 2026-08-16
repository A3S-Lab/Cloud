use super::DurableCellDeployment;
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct CreateDurableCellDeploymentWrite {
    pub deployment: DurableCellDeployment,
    pub idempotency: IdempotencyRequest,
}

impl CreateDurableCellDeploymentWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.deployment.validate()?;
        self.idempotency.validate()
    }
}

#[async_trait]
pub trait IDurableCellDeploymentRepository: Send + Sync {
    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError>;

    async fn create(
        &self,
        write: CreateDurableCellDeploymentWrite,
    ) -> Result<IdempotentWrite<DurableCellDeployment>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError>;
}
