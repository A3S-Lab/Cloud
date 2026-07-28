use crate::modules::assets::domain::{Asset, AssetState};
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use async_trait::async_trait;

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
}

pub fn validate_asset_repository_provision(asset: &Asset) -> Result<(), String> {
    asset.validate()?;
    if asset.state != AssetState::Active {
        return Err("archived Asset cannot provision a hosted Git repository".into());
    }
    Ok(())
}
