//! Stable, owner-published language for consumers of the Sources context.
//!
//! Consumer contexts depend on these immutable values instead of Sources
//! aggregates, repositories, or application handlers. These types physically
//! belong to the published layer, so the boundary is not an alias for the
//! owner's internal domain model.

mod build_recipe;
mod git_provider;
mod git_repository;
mod preview_source_revision_lifecycle;
mod pull_request_change_committed;
mod source_build_input;
mod source_revision_accepted;

pub use build_recipe::{BuildPlatform, BuildRecipe};
pub use git_provider::GitProvider;
pub use git_repository::GitRepository;
pub use preview_source_revision_lifecycle::{
    PreviewSourceRevisionLifecycleCommittedFact, PreviewSourceRevisionLifecycleState,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_MAX_BYTES,
};
pub use pull_request_change_committed::{
    PullRequestChangeCommittedFact, SourcePullRequestChangeKind,
    PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY, PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
};
pub use source_build_input::SourceBuildInputSnapshot;
pub(in crate::modules::sources) use source_build_input::ValidatedSourceBuildInputProjection;
pub use source_revision_accepted::{
    SourceRevisionAcceptedFact, SOURCE_REVISION_ACCEPTED_EVENT_KEY,
    SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
};
