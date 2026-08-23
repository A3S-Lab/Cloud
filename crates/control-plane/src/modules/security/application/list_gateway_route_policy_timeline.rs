use crate::modules::security::domain::{
    GatewayRoutePolicyTimelineCursor, GatewayRoutePolicyTimelinePage,
    IGatewayRoutePolicyTimelineRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_SECURITY_TIMELINE_LIMIT: usize = 50;
pub const MAXIMUM_SECURITY_TIMELINE_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct ListGatewayRoutePolicyTimeline {
    pub organization_id: OrganizationId,
    pub route_id: RouteId,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ListGatewayRoutePolicyTimeline {
    type Output = ApplicationResult<GatewayRoutePolicyTimelinePage>;
}

pub struct ListGatewayRoutePolicyTimelineHandler {
    repository: Arc<dyn IGatewayRoutePolicyTimelineRepository>,
}

impl ListGatewayRoutePolicyTimelineHandler {
    pub fn new(repository: Arc<dyn IGatewayRoutePolicyTimelineRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListGatewayRoutePolicyTimeline> for ListGatewayRoutePolicyTimelineHandler {
    fn execute(
        &self,
        query: ListGatewayRoutePolicyTimeline,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GatewayRoutePolicyTimelinePage>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if query.organization_id.as_uuid().is_nil() || query.route_id.as_uuid().is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "security timeline scope identifiers must not be nil".into(),
                )));
            }
            if query.limit == 0 || query.limit > MAXIMUM_SECURITY_TIMELINE_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "security timeline limit must be between 1 and {MAXIMUM_SECURITY_TIMELINE_LIMIT}"
                ))));
            }
            let cursor = match query
                .cursor
                .as_deref()
                .map(GatewayRoutePolicyTimelineCursor::parse)
            {
                Some(Ok(value)) => Some(value),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut entries = match repository
                .list_page(
                    query.organization_id,
                    query.route_id,
                    cursor,
                    query.limit + 1,
                )
                .await
            {
                Ok(entries) => entries,
                Err(error) => return Ok(Err(error.into())),
            };
            let next_cursor = (entries.len() > query.limit).then(|| {
                GatewayRoutePolicyTimelineCursor::after(&entries[query.limit - 1]).encode()
            });
            entries.truncate(query.limit);
            Ok(Ok(GatewayRoutePolicyTimelinePage {
                entries,
                next_cursor,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::events::{
        MCP_ROUTE_POLICY_CREATED_EVENT_KEY, MCP_ROUTE_POLICY_REVISED_EVENT_KEY,
    };
    use crate::modules::security::{
        GatewayRoutePolicyTimelineEntry, InMemoryGatewayRoutePolicyTimelineRepository,
        SecurityAuditCorrelation,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, EnvironmentId, ProjectId, Sha256Digest,
    };
    use a3s_boot::ModuleRef;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn entry(
        organization_id: OrganizationId,
        route_id: RouteId,
        revision: u64,
        occurred_at: chrono::DateTime<Utc>,
    ) -> GatewayRoutePolicyTimelineEntry {
        GatewayRoutePolicyTimelineEntry {
            event_id: Uuid::now_v7(),
            event_key: if revision == 1 {
                MCP_ROUTE_POLICY_CREATED_EVENT_KEY
            } else {
                MCP_ROUTE_POLICY_REVISED_EVENT_KEY
            }
            .into(),
            schema_version: 1,
            organization_id,
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            route_id,
            policy_revision: revision,
            policy_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("digest"),
            occurred_at: canonical_timestamp(occurred_at),
            correlation_id: Uuid::now_v7(),
            audit_correlation: SecurityAuditCorrelation::Missing,
            audit_record_id: None,
            actor_principal_id: None,
        }
    }

    #[tokio::test]
    async fn tenant_and_route_scope_are_keyset_paged_in_descending_owner_fact_order() {
        let repository = Arc::new(InMemoryGatewayRoutePolicyTimelineRepository::new());
        let organization_id = OrganizationId::new();
        let route_id = RouteId::new();
        let now = canonical_timestamp(Utc::now());
        for revision in 1..=3 {
            repository
                .register(entry(
                    organization_id,
                    route_id,
                    revision,
                    now + Duration::seconds(revision as i64),
                ))
                .await
                .expect("entry");
        }
        repository
            .register(entry(
                OrganizationId::new(),
                route_id,
                1,
                now + Duration::seconds(10),
            ))
            .await
            .expect("foreign tenant");

        let handler = ListGatewayRoutePolicyTimelineHandler::new(repository.clone());
        let first = handler
            .execute(
                ListGatewayRoutePolicyTimeline {
                    organization_id,
                    route_id,
                    cursor: None,
                    limit: 2,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("first page");
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.policy_revision)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        let second = handler
            .execute(
                ListGatewayRoutePolicyTimeline {
                    organization_id,
                    route_id,
                    cursor: first.next_cursor,
                    limit: 2,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("second page");
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].policy_revision, 1);
        assert!(second.next_cursor.is_none());
        assert_eq!(repository.query_count(), 2);
    }

    #[tokio::test]
    async fn invalid_scope_limit_and_cursor_fail_before_storage() {
        let repository = Arc::new(InMemoryGatewayRoutePolicyTimelineRepository::new());
        let handler = ListGatewayRoutePolicyTimelineHandler::new(repository.clone());
        for query in [
            ListGatewayRoutePolicyTimeline {
                organization_id: OrganizationId::from_uuid(Uuid::nil()),
                route_id: RouteId::new(),
                cursor: None,
                limit: 1,
            },
            ListGatewayRoutePolicyTimeline {
                organization_id: OrganizationId::new(),
                route_id: RouteId::from_uuid(Uuid::nil()),
                cursor: None,
                limit: 1,
            },
            ListGatewayRoutePolicyTimeline {
                organization_id: OrganizationId::new(),
                route_id: RouteId::new(),
                cursor: None,
                limit: 0,
            },
            ListGatewayRoutePolicyTimeline {
                organization_id: OrganizationId::new(),
                route_id: RouteId::new(),
                cursor: Some("untrusted".into()),
                limit: 1,
            },
        ] {
            assert!(handler
                .execute(query, CqrsContext::new(ModuleRef::new()))
                .await
                .expect("framework")
                .is_err());
        }
        assert_eq!(repository.query_count(), 0);
    }
}
