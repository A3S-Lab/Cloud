//! Deliberate public Audit contracts.
//!
//! Concrete `infrastructure` and `presentation` modules stay crate-private.
//! Selected adapters, repositories, response DTOs, and the module wiring
//! surface are re-exported from the bounded-context root so consumers never
//! depend on an outer-layer module path.

pub use super::infrastructure::{InMemoryAuditRecordRepository, PostgresAuditRecordRepository};
pub use super::presentation::{
    audit_query_controller, AuditExportManifestBundleResponse, AuditExportResponse,
    AuditModule, AuditRecordPageResponse, AuditRetentionStatusResponse,
};
