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
    AssetReleaseWrite, AssetReleaseWriteReference, AssetWrite, AssetWriteReference,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfileBinding, TransitionAssetReleaseWrite, TransitionAssetWrite,
};
pub use services::{
    validate_asset_repository_provision, AssetGitRepository, AssetGitRepositoryError,
    AssetGitRepositoryWrite, IAssetGitRepository, DEFAULT_ASSET_BRANCH,
};
pub use value_objects::{
    AssetReleaseArtifact, AssetReleaseArtifactKind, AssetReleaseVersion, McpServiceProfile,
    McpServiceProfileSpec, SKILL_BUNDLE_MEDIA_TYPE,
};

#[cfg(test)]
mod tests;
