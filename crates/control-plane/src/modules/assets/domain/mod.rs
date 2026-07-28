pub mod entities;
pub mod events;
pub mod repositories;
pub mod value_objects;

pub use entities::{Asset, AssetKind, AssetRelease, AssetReleaseState, AssetState};
pub use events::{
    AssetArchived, AssetCreated, AssetReleaseDrafted, AssetReleasePublished, AssetReleaseYanked,
};
pub use repositories::{
    AssetReleaseWrite, AssetReleaseWriteReference, AssetWrite, AssetWriteReference,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, TransitionAssetReleaseWrite,
    TransitionAssetWrite,
};
pub use value_objects::{
    AssetReleaseArtifact, AssetReleaseArtifactKind, AssetReleaseVersion, SKILL_BUNDLE_MEDIA_TYPE,
};

#[cfg(test)]
mod tests;
