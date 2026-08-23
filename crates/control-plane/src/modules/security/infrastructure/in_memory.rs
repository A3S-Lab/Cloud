use crate::modules::security::domain::{
    GatewayRoutePolicyTimelineCursor, GatewayRoutePolicyTimelineEntry,
    IGatewayRoutePolicyTimelineRepository,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, RepositoryError, RouteId,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryGatewayRoutePolicyTimelineRepository {
    entries: RwLock<Vec<GatewayRoutePolicyTimelineEntry>>,
    query_count: AtomicUsize,
}

impl InMemoryGatewayRoutePolicyTimelineRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(
        &self,
        mut entry: GatewayRoutePolicyTimelineEntry,
    ) -> Result<(), RepositoryError> {
        entry.occurred_at = canonical_timestamp(entry.occurred_at);
        entry.validate().map_err(RepositoryError::Storage)?;
        let mut entries = self.entries.write().await;
        if entries
            .iter()
            .any(|existing| existing.event_id == entry.event_id)
        {
            return Err(RepositoryError::Conflict(
                "security timeline event ID is already in use".into(),
            ));
        }
        entries.push(entry);
        Ok(())
    }

    pub fn query_count(&self) -> usize {
        self.query_count.load(AtomicOrdering::Relaxed)
    }
}

#[async_trait]
impl IGatewayRoutePolicyTimelineRepository for InMemoryGatewayRoutePolicyTimelineRepository {
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
        after: Option<GatewayRoutePolicyTimelineCursor>,
        limit: usize,
    ) -> Result<Vec<GatewayRoutePolicyTimelineEntry>, RepositoryError> {
        self.query_count.fetch_add(1, AtomicOrdering::Relaxed);
        let mut entries = self
            .entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.organization_id == organization_id && entry.route_id == route_id)
            .filter(|entry| {
                after.is_none_or(|cursor| {
                    entry.occurred_at < cursor.occurred_at
                        || (entry.occurred_at == cursor.occurred_at
                            && entry.event_id < cursor.event_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| right.event_id.cmp(&left.event_id))
        });
        entries.truncate(limit.max(1));
        Ok(entries)
    }
}
