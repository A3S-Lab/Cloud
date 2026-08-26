//! Stable, owner-published language for consumers of Developer Workflows.
//!
//! These immutable values contain no Developer Workflows aggregate behavior
//! and never alias another bounded context's internal model.

mod pull_request_preview_lifecycle;

pub use pull_request_preview_lifecycle::{
    PullRequestPreviewLifecycleCommitted, PREVIEW_MAX_ACTIVE_PER_POLICY, PREVIEW_MAX_CPU_MILLIS,
    PREVIEW_MAX_LIFETIME_SECONDS, PREVIEW_MAX_MEMORY_BYTES, PREVIEW_MAX_STORAGE_BYTES,
    PREVIEW_MAX_WORKLOADS, PREVIEW_MIN_LIFETIME_SECONDS, PREVIEW_MIN_MEMORY_BYTES,
    PREVIEW_MIN_STORAGE_BYTES, PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
    PULL_REQUEST_PREVIEW_LIFECYCLE_MAX_BYTES,
};
