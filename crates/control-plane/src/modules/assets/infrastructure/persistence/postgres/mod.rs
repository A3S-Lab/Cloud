mod queries;
mod rows;
mod writes;

#[cfg(test)]
mod typed_orm_tests;

use crate::modules::assets::domain::{
    Asset, AssetRelease, AssetReleaseWrite, AssetWrite, CreateAssetReleaseWrite, CreateAssetWrite,
    IAssetRepository, TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;

#[derive(Clone)]
pub struct PostgresAssetRepository {
    executor: PostgresExecutor,
}

impl PostgresAssetRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAssetRepository for PostgresAssetRepository {
    async fn create_asset(&self, bundle: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        writes::create_asset(&self.executor, bundle).await
    }

    async fn transition_asset(
        &self,
        bundle: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError> {
        writes::transition_asset(&self.executor, bundle).await
    }

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError> {
        queries::find_asset(&self.executor, organization_id, asset_id).await
    }

    async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError> {
        queries::list_assets(&self.executor, organization_id).await
    }

    async fn create_release(
        &self,
        bundle: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        writes::create_release(&self.executor, bundle).await
    }

    async fn transition_release(
        &self,
        bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        writes::transition_release(&self.executor, bundle).await
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError> {
        queries::find_release(&self.executor, organization_id, asset_id, asset_release_id).await
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        queries::list_releases(&self.executor, organization_id, asset_id).await
    }
}
