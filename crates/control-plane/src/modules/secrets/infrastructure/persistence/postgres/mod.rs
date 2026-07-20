mod queries;
mod rows;
mod writes;

use crate::modules::secrets::domain::{
    CreateSecretWrite, ISecretRepository, RotateSecretWrite, Secret, SecretVersion, SecretWrite,
    TransitionSecretVersion,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;

#[derive(Clone)]
pub struct PostgresSecretRepository {
    executor: PostgresExecutor,
}

impl PostgresSecretRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl ISecretRepository for PostgresSecretRepository {
    async fn replay_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<SecretWrite>, RepositoryError> {
        writes::replay(&self.executor, organization_id, idempotency).await
    }

    async fn create(&self, bundle: CreateSecretWrite) -> Result<SecretWrite, RepositoryError> {
        writes::create(&self.executor, bundle).await
    }

    async fn rotate(&self, bundle: RotateSecretWrite) -> Result<SecretWrite, RepositoryError> {
        writes::rotate(&self.executor, bundle).await
    }

    async fn transition_version(
        &self,
        bundle: TransitionSecretVersion,
    ) -> Result<SecretWrite, RepositoryError> {
        writes::transition_version(&self.executor, bundle).await
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Secret, RepositoryError> {
        queries::find(&self.executor, organization_id, secret_id).await
    }

    async fn find_version(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
        version: u64,
    ) -> Result<SecretVersion, RepositoryError> {
        queries::find_version(&self.executor, organization_id, secret_id, version).await
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Secret>, RepositoryError> {
        queries::list(&self.executor, organization_id, project_id, environment_id).await
    }

    async fn list_versions(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Vec<SecretVersion>, RepositoryError> {
        queries::list_versions(&self.executor, organization_id, secret_id).await
    }
}
