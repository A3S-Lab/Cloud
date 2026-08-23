use super::{GatewayRoutePolicyTimelineCursor, GatewayRoutePolicyTimelineEntry};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, RouteId};
use async_trait::async_trait;

#[async_trait]
pub trait IGatewayRoutePolicyTimelineRepository: Send + Sync {
    /// Returns one stable page ordered by owner-fact occurrence and event ID descending.
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
        after: Option<GatewayRoutePolicyTimelineCursor>,
        limit: usize,
    ) -> Result<Vec<GatewayRoutePolicyTimelineEntry>, RepositoryError>;
}
