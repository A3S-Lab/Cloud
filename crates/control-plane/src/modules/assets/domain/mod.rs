pub mod entities;
pub mod events;
pub mod repositories;
pub mod services;
pub mod value_objects;

pub use entities::{Asset, AssetKind, AssetRelease, AssetReleaseState, AssetState};
pub use events::{
    AssetArchived, AssetCreated, AssetReleaseDrafted, AssetReleasePublished, AssetReleaseYanked,
};
pub use repositories::{
    AcquireAssetGitWriteLease, AssetGitRepositoryControlError, AssetGitWriteJournal,
    AssetGitWriteLease, AssetGitWriteOperation, AssetGitWriteRecovery, AssetReleaseWrite,
    AssetReleaseWriteReference, AssetWrite, AssetWriteReference, ClaimAssetGitWriteRecovery,
    CompleteAssetGitWriteLease, CreateAssetReleaseWrite, CreateAssetWrite,
    IAssetGitRepositoryControl, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfileBinding, TransitionAssetReleaseWrite, TransitionAssetWrite,
};
pub use services::{
    validate_asset_repository_mutation, AssetGitRepository, AssetGitRepositoryError,
    AssetGitRepositoryWrite, IAssetGitRepository, DEFAULT_ASSET_BRANCH,
};
pub use value_objects::{
    AssetGitBackup, AssetGitBuildInput, AssetGitRpcLimits, AssetGitRpcResponse, AssetGitService,
    AssetManifestAdmission, AssetReleaseArtifact, AssetReleaseArtifactKind, AssetReleaseProvenance,
    AssetReleaseVersion, McpServiceProfile, McpServiceProfileSpec, SKILL_BUNDLE_MEDIA_TYPE,
};

#[cfg(test)]
mod tests;
