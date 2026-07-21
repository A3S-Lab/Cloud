use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceSubscriptionId,
};
use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;

pub struct CreateGithubRepositorySubscription {
    pub subscription: GithubRepositorySubscription,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

pub struct DeactivateGithubRepositorySubscription {
    pub subscription: GithubRepositorySubscription,
    pub previous_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[async_trait]
pub trait ISourceSubscriptionRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
    ) -> Result<Option<GithubRepositorySubscription>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GithubRepositorySubscription>, RepositoryError>;

    async fn deactivate(
        &self,
        request: DeactivateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError>;
}
