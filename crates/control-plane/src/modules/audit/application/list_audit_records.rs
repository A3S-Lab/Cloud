use crate::modules::audit::domain::{
    AuditRecordCursor, AuditRecordFilter, AuditRecordPage, IAuditRecordRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_AUDIT_RECORD_LIMIT: usize = 50;
pub const MAXIMUM_AUDIT_RECORD_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct ListAuditRecords {
    pub organization_id: OrganizationId,
    pub filter: AuditRecordFilter,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ListAuditRecords {
    type Output = ApplicationResult<AuditRecordPage>;
}

pub struct ListAuditRecordsHandler {
    repository: Arc<dyn IAuditRecordRepository>,
}

impl ListAuditRecordsHandler {
    pub fn new(repository: Arc<dyn IAuditRecordRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListAuditRecords> for ListAuditRecordsHandler {
    fn execute(
        &self,
        query: ListAuditRecords,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AuditRecordPage>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if query.limit == 0 || query.limit > MAXIMUM_AUDIT_RECORD_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "audit record limit must be between 1 and {MAXIMUM_AUDIT_RECORD_LIMIT}"
                ))));
            }
            if let Err(error) = query.filter.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let cursor = match query.cursor.as_deref().map(AuditRecordCursor::parse) {
                Some(Ok(value)) => Some(value),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut records = match repository
                .list_page(
                    query.organization_id,
                    &query.filter,
                    cursor,
                    query.limit + 1,
                )
                .await
            {
                Ok(records) => records,
                Err(error) => return Ok(Err(error.into())),
            };
            let next_cursor = (records.len() > query.limit)
                .then(|| AuditRecordCursor::after(&records[query.limit - 1]).encode());
            records.truncate(query.limit);
            Ok(Ok(AuditRecordPage {
                records,
                next_cursor,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::audit::{
        AuditAttributionStatus, AuditRecord, InMemoryAuditRecordRepository,
    };
    use crate::modules::shared_kernel::domain::PrincipalId;
    use a3s_boot::ModuleRef;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    #[tokio::test]
    async fn filters_and_paginates_in_stable_descending_order() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let request_id = Uuid::now_v7();
        let now = Utc::now();
        for offset in 0..3 {
            repository
                .register(AuditRecord {
                    id: Uuid::now_v7(),
                    organization_id,
                    actor_principal_id: Some(actor),
                    action: if offset == 1 {
                        "identity.membership.revoked".into()
                    } else {
                        "identity.membership.created".into()
                    },
                    aggregate_id: Uuid::now_v7(),
                    occurred_at: now + Duration::seconds(offset),
                    request_id,
                    project_id: None,
                    environment_id: None,
                    attribution_profile_id: None,
                    attribution_status: AuditAttributionStatus::NotApplicable,
                })
                .await
                .expect("audit record");
        }
        let handler = ListAuditRecordsHandler::new(repository);
        let context = CqrsContext::new(ModuleRef::new());
        let first = handler
            .execute(
                ListAuditRecords {
                    organization_id,
                    filter: AuditRecordFilter {
                        actor_principal_id: Some(actor),
                        request_id: Some(request_id),
                        ..AuditRecordFilter::default()
                    },
                    cursor: None,
                    limit: 2,
                },
                context.clone(),
            )
            .await
            .expect("framework")
            .expect("first page");
        assert_eq!(first.records.len(), 2);
        assert!(first.records[0].occurred_at > first.records[1].occurred_at);
        let second = handler
            .execute(
                ListAuditRecords {
                    organization_id,
                    filter: AuditRecordFilter {
                        actor_principal_id: Some(actor),
                        request_id: Some(request_id),
                        ..AuditRecordFilter::default()
                    },
                    cursor: first.next_cursor,
                    limit: 2,
                },
                context,
            )
            .await
            .expect("framework")
            .expect("second page");
        assert_eq!(second.records.len(), 1);
        assert!(second.next_cursor.is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_limits_filters_and_cursors_before_storage() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let handler = ListAuditRecordsHandler::new(repository.clone());
        for query in [
            ListAuditRecords {
                organization_id: OrganizationId::new(),
                filter: AuditRecordFilter::default(),
                cursor: None,
                limit: 0,
            },
            ListAuditRecords {
                organization_id: OrganizationId::new(),
                filter: AuditRecordFilter {
                    action: Some("invalid action".into()),
                    ..AuditRecordFilter::default()
                },
                cursor: None,
                limit: 1,
            },
            ListAuditRecords {
                organization_id: OrganizationId::new(),
                filter: AuditRecordFilter::default(),
                cursor: Some("invalid".into()),
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
