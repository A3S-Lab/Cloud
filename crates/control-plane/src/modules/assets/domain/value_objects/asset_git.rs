use crate::modules::assets::domain::AssetKind;
use crate::modules::shared_kernel::domain::{
    AssetReleaseId, BuildRunId, GitCommitSha, Sha256Digest,
};
use crate::modules::sources::domain::BuildRecipe;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetGitService {
    UploadPack,
    ReceivePack,
}

impl AssetGitService {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }

    pub const fn git_subcommand(self) -> &'static str {
        match self {
            Self::UploadPack => "upload-pack",
            Self::ReceivePack => "receive-pack",
        }
    }

    pub const fn request_media_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-request",
            Self::ReceivePack => "application/x-git-receive-pack-request",
        }
    }

    pub const fn result_media_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-result",
            Self::ReceivePack => "application/x-git-receive-pack-result",
        }
    }

    pub const fn advertisement_media_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-advertisement",
            Self::ReceivePack => "application/x-git-receive-pack-advertisement",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitRpcResponse {
    pub body: Vec<u8>,
    pub repository_bytes: u64,
    pub refs_digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetGitRpcLimits {
    pub maximum_input_bytes: u64,
    pub maximum_repository_bytes: u64,
}

impl AssetGitRpcLimits {
    pub fn validate(self) -> Result<Self, String> {
        if self.maximum_input_bytes == 0 || self.maximum_repository_bytes == 0 {
            return Err("Asset Git RPC limits must be positive".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitBackup {
    pub object_key: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
    pub refs_digest: Sha256Digest,
    pub created_at: DateTime<Utc>,
}

impl AssetGitBackup {
    pub fn validate(&self) -> Result<(), String> {
        let path = Path::new(&self.object_key);
        if self.object_key.is_empty()
            || self.object_key.len() > 4096
            || self.object_key.contains(['\\', '\0', '\r', '\n'])
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || self.size_bytes == 0
        {
            return Err("Asset Git backup identity is invalid".into());
        }
        Sha256Digest::parse(self.digest.as_str())?;
        Sha256Digest::parse(self.refs_digest.as_str())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetManifestAdmission {
    pub commit_sha: GitCommitSha,
    pub manifest_digest: Sha256Digest,
    pub kind: AssetKind,
    pub build_recipe: Option<BuildRecipe>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitBuildInput {
    pub build_run_id: BuildRunId,
    pub commit_sha: GitCommitSha,
    pub content_digest: Sha256Digest,
    pub size_bytes: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGitReleaseBundle {
    pub asset_release_id: AssetReleaseId,
    pub commit_sha: GitCommitSha,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
    pub path: PathBuf,
}

impl AssetGitBuildInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.build_run_id.as_uuid().is_nil()
            || self.size_bytes == 0
            || self.path.as_os_str().is_empty()
        {
            return Err("Asset Git build input identity is invalid".into());
        }
        GitCommitSha::parse(self.commit_sha.as_str())?;
        Sha256Digest::parse(self.content_digest.as_str())?;
        Ok(())
    }
}

impl AssetGitReleaseBundle {
    pub fn validate(&self) -> Result<(), String> {
        if self.asset_release_id.as_uuid().is_nil()
            || self.size_bytes == 0
            || self.path.as_os_str().is_empty()
        {
            return Err("Asset Git release bundle identity is invalid".into());
        }
        GitCommitSha::parse(self.commit_sha.as_str())?;
        Sha256Digest::parse(self.digest.as_str())?;
        Ok(())
    }
}

impl AssetManifestAdmission {
    pub fn validate_for(&self, kind: AssetKind) -> Result<(), String> {
        GitCommitSha::parse(self.commit_sha.as_str())?;
        Sha256Digest::parse(self.manifest_digest.as_str())?;
        if self.kind != kind {
            return Err("Asset manifest kind does not match its Asset".into());
        }
        if let Some(recipe) = &self.build_recipe {
            if self.kind == AssetKind::Skill || recipe.clone().validate()? != *recipe {
                return Err("Asset manifest build recipe does not match its Asset kind".into());
            }
        }
        Ok(())
    }
}
