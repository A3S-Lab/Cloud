//! Stable, owner-published language for consumers of the Assets context.
//!
//! These immutable snapshots expose only identities and values admitted by
//! Assets. Consumers do not receive Asset aggregates, release lifecycle state,
//! hosted Git repositories, or persistence contracts.

mod hosted_build_input;

pub use hosted_build_input::HostedAssetBuildInputSnapshot;
pub(in crate::modules::assets) use hosted_build_input::ValidatedHostedAssetBuildInputProjection;
