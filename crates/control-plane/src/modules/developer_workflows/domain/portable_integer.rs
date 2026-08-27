/// Largest integer that remains exact across A3S ACL, JSON, OpenAPI, and
/// JavaScript clients.
///
/// Developer Workflows persists several counters as signed 64-bit integers,
/// but persistence capacity is not the public contract. Keeping one bounded-
/// context constant prevents accepted revisions and Preview lifecycle facts
/// from becoming lossy when they cross a public adapter.
pub const MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
