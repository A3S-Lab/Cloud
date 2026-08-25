//! Stable, owner-published language for consumers of Developer Workflows.
//!
//! These immutable values contain no Developer Workflows aggregate behavior
//! and never alias another bounded context's internal model.

mod pull_request_preview_lifecycle;

pub use pull_request_preview_lifecycle::{
    PullRequestPreviewLifecycleCommitted, PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
};
