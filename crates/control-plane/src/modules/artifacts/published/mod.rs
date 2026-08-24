//! Stable facts published by the Artifacts bounded context.
//!
//! Consumers depend on these immutable values instead of `BuildRun`, build
//! repositories, or Artifacts persistence. Construction remains private to
//! Artifacts so deserialized facts must pass the same closed validation.

mod hosted_build_outcome;

pub(in crate::modules::artifacts) use hosted_build_outcome::ValidatedHostedBuildOutcomeProjection;
pub use hosted_build_outcome::{
    HostedBuildArtifact, HostedBuildOutcome, HOSTED_BUILD_OUTCOME_EVENT_KEY,
    HOSTED_BUILD_OUTCOME_SCHEMA,
};
