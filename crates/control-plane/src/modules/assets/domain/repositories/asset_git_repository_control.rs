use crate::modules::assets::domain::{Asset, AssetGitBackup};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetGitWriteOperation {
    ReceivePack,
    Backup,
    Restore,
}

impl AssetGitWriteOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReceivePack => "receive_pack",
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }

    pub const fn audit_action(self) -> &'static str {
        match self {
            Self::ReceivePack => "asset.repository.pushed",
            Self::Backup => "asset.repository.backed-up",
            Self::Restore => "asset.repository.restored",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AcquireAssetGitWriteLease {
    pub asset: Asset,
    pub lease_id: Uuid,
    pub operation: AssetGitWriteOperation,
    pub actor_id: Uuid,
    pub request_id: Uuid,
    pub observed_bytes: u64,
    pub default_quota_bytes: u64,
    pub acquired_at: DateTime<Utc>,
    pub leased_until: DateTime<Utc>,
}

impl AcquireAssetGitWriteLease {
    pub fn validate(&self) -> Result<(), String> {
        self.asset.validate()?;
        if self.lease_id.is_nil()
            || self.actor_id.is_nil()
            || self.request_id.is_nil()
            || self.default_quota_bytes == 0
            || self.leased_until <= self.acquired_at
        {
            return Err("Asset Git write lease request is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitWriteLease {
    pub organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    pub asset_id: crate::modules::shared_kernel::domain::AssetId,
    pub lease_id: Uuid,
    pub operation: AssetGitWriteOperation,
    pub actor_id: Uuid,
    pub request_id: Uuid,
    pub quota_bytes: u64,
    pub observed_bytes: u64,
    pub leased_until: DateTime<Utc>,
    pub recovery: bool,
}

impl AssetGitWriteLease {
    pub fn validate_for(&self, asset: &Asset) -> Result<(), String> {
        asset.validate()?;
        if self.organization_id != asset.organization_id
            || self.asset_id != asset.id
            || self.lease_id.is_nil()
            || self.actor_id.is_nil()
            || self.request_id.is_nil()
            || self.quota_bytes == 0
        {
            return Err("Asset Git write lease does not match its repository".into());
        }
        Ok(())
    }

    pub const fn journal(&self) -> AssetGitWriteJournal {
        AssetGitWriteJournal {
            organization_id: self.organization_id,
            asset_id: self.asset_id,
            lease_id: self.lease_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetGitWriteJournal {
    pub organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    pub asset_id: crate::modules::shared_kernel::domain::AssetId,
    pub lease_id: Uuid,
}

impl AssetGitWriteJournal {
    pub fn validate_for(&self, asset: &Asset) -> Result<(), String> {
        asset.validate()?;
        if self.organization_id != asset.organization_id
            || self.asset_id != asset.id
            || self.lease_id.is_nil()
        {
            return Err("Asset Git write journal does not match its repository".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ClaimAssetGitWriteRecovery {
    pub asset: Asset,
    pub claimed_at: DateTime<Utc>,
    pub leased_until: DateTime<Utc>,
}

impl ClaimAssetGitWriteRecovery {
    pub fn validate(&self) -> Result<(), String> {
        self.asset.validate()?;
        if self.leased_until <= self.claimed_at {
            return Err("Asset Git write recovery lease is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetGitWriteRecovery {
    Active,
    Rollback(AssetGitWriteLease),
    Cleanup(AssetGitWriteJournal),
}

#[derive(Debug, Clone)]
pub struct CompleteAssetGitWriteLease {
    pub lease: AssetGitWriteLease,
    pub observed_bytes: u64,
    pub refs_digest: Sha256Digest,
    pub backup: Option<AssetGitBackup>,
    pub completed_at: DateTime<Utc>,
}

impl CompleteAssetGitWriteLease {
    pub fn validate(&self) -> Result<(), String> {
        Sha256Digest::parse(self.refs_digest.as_str())?;
        if self.lease.recovery {
            return Err("Asset Git recovery lease cannot complete a write".into());
        }
        if self.observed_bytes > self.lease.quota_bytes {
            return Err("Asset Git repository exceeds its durable quota".into());
        }
        match (&self.backup, self.lease.operation) {
            (Some(backup), AssetGitWriteOperation::Backup) => backup.validate(),
            (None, AssetGitWriteOperation::ReceivePack | AssetGitWriteOperation::Restore) => Ok(()),
            _ => Err("Asset Git write completion does not match its operation".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetGitRepositoryControlError {
    #[error("hosted Git repository control request is invalid: {0}")]
    Invalid(String),
    #[error("hosted Git repository control record was not found")]
    NotFound,
    #[error("hosted Git repository already has an active writer")]
    Busy,
    #[error(
        "hosted Git repository exceeds its {quota_bytes}-byte quota with {observed_bytes} bytes"
    )]
    QuotaExceeded {
        quota_bytes: u64,
        observed_bytes: u64,
    },
    #[error("hosted Git repository write lease is stale")]
    StaleLease,
    #[error("hosted Git repository requires write-journal recovery")]
    RecoveryRequired,
    #[error("hosted Git repository control storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IAssetGitRepositoryControl: Send + Sync {
    async fn claim_write_recovery(
        &self,
        request: ClaimAssetGitWriteRecovery,
    ) -> Result<Option<AssetGitWriteRecovery>, AssetGitRepositoryControlError>;

    async fn acquire_write(
        &self,
        request: AcquireAssetGitWriteLease,
    ) -> Result<AssetGitWriteLease, AssetGitRepositoryControlError>;

    async fn complete_write(
        &self,
        completion: CompleteAssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError>;

    async fn abandon_write(
        &self,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError>;

    async fn settle_write(
        &self,
        journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryControlError>;
}
