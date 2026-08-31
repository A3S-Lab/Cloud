//! Stable, owner-published language for consumers of the Assets context.
//!
//! These immutable snapshots expose only identities and values admitted by
//! Assets. Consumers do not receive Asset aggregates, release lifecycle state,
//! hosted Git repositories, or persistence contracts.

mod hosted_build_input;
mod hosted_build_requested;

pub(in crate::modules::assets) use hosted_build_input::ValidatedHostedAssetBuildInputProjection;
pub use hosted_build_input::{HostedAgentReleaseTemplate, HostedAssetBuildInputSnapshot};
pub use hosted_build_requested::{
    HostedAssetBuildRequestedFact, HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY,
    HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION,
};
