mod git_controls;
mod hosted_publications;
mod mcp_profiles;
mod queries;
mod rows;
mod writes;

#[cfg(test)]
mod typed_orm_tests;

use crate::modules::assets::domain::{
    AcquireAssetGitWriteLease, Asset, AssetGitRepositoryControlError, AssetGitWriteJournal,
    AssetGitWriteLease, AssetGitWriteRecovery, AssetRelease, AssetReleaseWrite, AssetWrite,
    ClaimAssetGitWriteRecovery, CompleteAssetGitWriteLease, CreateAssetReleaseWrite,
    CreateAssetWrite, IAssetGitRepositoryControl, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfileBinding, TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;

pub(crate) use hosted_publications::{
    apply_hosted_release, plan_hosted_release, verify_hosted_release_unpublished, HostedReleasePlan,
};

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

#[async_trait]
impl IAssetGitRepositoryControl for PostgresAssetRepository {
    async fn claim_write_recovery(
        &self,
        request: ClaimAssetGitWriteRecovery,
    ) -> Result<Option<AssetGitWriteRecovery>, AssetGitRepositoryControlError> {
        git_controls::claim_write_recovery(&self.executor, request).await
    }

    async fn acquire_write(
        &self,
        request: AcquireAssetGitWriteLease,
    ) -> Result<AssetGitWriteLease, AssetGitRepositoryControlError> {
        git_controls::acquire_write(&self.executor, request).await
    }

    async fn complete_write(
        &self,
        completion: CompleteAssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError> {
        git_controls::complete_write(&self.executor, completion).await
    }

    async fn abandon_write(
        &self,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError> {
        git_controls::abandon_write(&self.executor, lease).await
    }

    async fn settle_write(
        &self,
        journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryControlError> {
        git_controls::settle_write(&self.executor, journal).await
    }
}

#[async_trait]
impl IMcpServiceProfileRepository for PostgresAssetRepository {
    async fn bind_mcp_service_profile(
        &self,
        binding: McpServiceProfileBinding,
    ) -> Result<McpServiceProfileBinding, RepositoryError> {
        mcp_profiles::bind(&self.executor, binding).await
    }

    async fn find_mcp_service_profile(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<McpServiceProfileBinding>, RepositoryError> {
        mcp_profiles::find(&self.executor, organization_id, asset_id, asset_release_id).await
    }
}
