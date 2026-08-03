use crate::modules::assets::domain::{
    Asset, AssetGitBackup, AssetGitBuildInput, AssetGitRpcLimits, AssetGitRpcResponse,
    AssetGitService, AssetGitWriteJournal, AssetGitWriteLease, AssetManifestAdmission, AssetState,
};
use crate::modules::shared_kernel::domain::{
    AssetId, BuildRunId, GitCommitSha, OrganizationId, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub const DEFAULT_ASSET_BRANCH: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitRepository {
    organization_id: OrganizationId,
    asset_id: AssetId,
    default_branch: String,
}

impl AssetGitRepository {
    pub fn for_asset(asset: &Asset) -> Result<Self, String> {
        asset.validate()?;
        Ok(Self {
            organization_id: asset.organization_id,
            asset_id: asset.id,
            default_branch: DEFAULT_ASSET_BRANCH.into(),
        })
    }

    pub fn validate_for(&self, asset: &Asset) -> Result<(), String> {
        asset.validate()?;
        if self.organization_id != asset.organization_id
            || self.asset_id != asset.id
            || self.default_branch != DEFAULT_ASSET_BRANCH
        {
            return Err("hosted Git repository identity does not match its Asset".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitRepositoryWrite {
    pub repository: AssetGitRepository,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetGitRepositoryError {
    #[error("hosted Git repository request is invalid: {0}")]
    Invalid(String),
    #[error("hosted Git repository was not found")]
    NotFound,
    #[error("hosted Git repository failed integrity validation: {0}")]
    Integrity(String),
    #[error("hosted Git repository request exceeds its configured quota")]
    QuotaExceeded,
    #[error("hosted Git repository backup support is unavailable")]
    BackupUnavailable,
    #[error("hosted Git repository storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IAssetGitRepository: Send + Sync {
    async fn provision(
        &self,
        asset: &Asset,
    ) -> Result<AssetGitRepositoryWrite, AssetGitRepositoryError>;

    async fn inspect(&self, asset: &Asset) -> Result<AssetGitRepository, AssetGitRepositoryError>;

    async fn prepare_write(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError>;

    async fn rollback_write(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError>;

    async fn settle_write(
        &self,
        asset: &Asset,
        journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryError>;

    async fn advertise(
        &self,
        asset: &Asset,
        service: AssetGitService,
    ) -> Result<Vec<u8>, AssetGitRepositoryError>;

    async fn execute_rpc(
        &self,
        asset: &Asset,
        service: AssetGitService,
        request: Vec<u8>,
        limits: AssetGitRpcLimits,
        write_lease: Option<&AssetGitWriteLease>,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError>;

    async fn repository_bytes(&self, asset: &Asset) -> Result<u64, AssetGitRepositoryError>;

    async fn refs_digest(&self, asset: &Asset) -> Result<Sha256Digest, AssetGitRepositoryError>;

    async fn create_backup(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        created_at: DateTime<Utc>,
    ) -> Result<AssetGitBackup, AssetGitRepositoryError>;

    async fn restore_backup(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        backup: &AssetGitBackup,
        maximum_repository_bytes: u64,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError>;

    async fn admit_manifest(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
    ) -> Result<AssetManifestAdmission, AssetGitRepositoryError>;

    async fn prepare_build_input(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
        build_run_id: BuildRunId,
    ) -> Result<AssetGitBuildInput, AssetGitRepositoryError>;

    async fn remove_build_input(
        &self,
        build_run_id: BuildRunId,
    ) -> Result<(), AssetGitRepositoryError>;
}

pub fn validate_asset_repository_mutation(asset: &Asset) -> Result<(), String> {
    asset.validate()?;
    if asset.state != AssetState::Active {
        return Err("archived Asset repository is read-only".into());
    }
    Ok(())
}
