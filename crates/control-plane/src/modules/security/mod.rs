mod application;
mod domain;
mod infrastructure;
mod presentation;

use a3s_orm::PostgresExecutor;
use infrastructure::PostgresGatewayRoutePolicyTimelineRepository;
use std::sync::Arc;

pub use application::{
    ListGatewayRoutePolicyTimeline, ListGatewayRoutePolicyTimelineHandler,
    DEFAULT_SECURITY_TIMELINE_LIMIT, MAXIMUM_SECURITY_TIMELINE_LIMIT,
};
pub use domain::{
    GatewayRoutePolicyTimelineCursor, GatewayRoutePolicyTimelineEntry,
    GatewayRoutePolicyTimelinePage, IGatewayRoutePolicyTimelineRepository,
    SecurityAuditCorrelation,
};
pub(crate) use presentation::{GatewayRoutePolicyTimelinePageResponse, SecurityModule};

/// Builds the production persistence adapter inside the Security owner
/// boundary and exposes only its domain port to process composition and
/// retained conformance gates.
pub(crate) fn security_persistence_adapter(
    executor: PostgresExecutor,
) -> Arc<dyn IGatewayRoutePolicyTimelineRepository> {
    Arc::new(PostgresGatewayRoutePolicyTimelineRepository::new(executor))
}

#[cfg(test)]
pub(crate) use infrastructure::InMemoryGatewayRoutePolicyTimelineRepository;
