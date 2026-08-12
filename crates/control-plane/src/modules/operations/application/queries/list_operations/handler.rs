use super::ListOperations;
use crate::modules::operations::application::resource_access::IOperationResourceAccess;
use crate::modules::operations::domain::entities::OperationRecord;
use crate::modules::operations::domain::repositories::{IOperationRepository, OperationListCursor};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::sync::Arc;

const OPERATION_VISIBILITY_CONCURRENCY: usize = 16;

pub struct ListOperationsHandler {
    repository: Arc<dyn IOperationRepository>,
    resource_access: Arc<dyn IOperationResourceAccess>,
}

impl ListOperationsHandler {
    pub(crate) fn new(
        repository: Arc<dyn IOperationRepository>,
        resource_access: Arc<dyn IOperationResourceAccess>,
    ) -> Self {
        Self {
            repository,
            resource_access,
        }
    }
}

impl QueryHandler<ListOperations> for ListOperationsHandler {
    fn execute(
        &self,
        query: ListOperations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<OperationRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        let resource_access = Arc::clone(&self.resource_access);
        Box::pin(async move {
            let requested_limit = query.limit.clamp(1, 200);
            if query.resource_access.is_organization_wide() {
                return Ok(repository
                    .list(query.organization_id, requested_limit)
                    .await
                    .map_err(Into::into));
            }

            let mut visible_records = Vec::with_capacity(requested_limit);
            let mut after = None;
            loop {
                let records = match repository
                    .list_page(query.organization_id, after, 200)
                    .await
                {
                    Ok(records) => records,
                    Err(error) => return Ok(Err(error.into())),
                };
                let page_len = records.len();
                let next = records.last().map(OperationListCursor::after);
                let checked = match stream::iter(records.into_iter().map(|record| {
                    let resource_access = Arc::clone(&resource_access);
                    let evaluator = query.resource_access.clone();
                    async move {
                        resource_access
                            .subject_is_visible(
                                record.request.organization_id,
                                &record.request.subject,
                                &evaluator,
                            )
                            .await
                            .map(|visible| (record, visible))
                    }
                }))
                .buffered(OPERATION_VISIBILITY_CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await
                {
                    Ok(checked) => checked,
                    Err(error) => return Ok(Err(error)),
                };
                visible_records.extend(
                    checked
                        .into_iter()
                        .filter_map(|(record, visible)| visible.then_some(record))
                        .take(requested_limit - visible_records.len()),
                );
                if visible_records.len() == requested_limit || page_len < 200 {
                    return Ok(Ok(visible_records));
                }
                after = next;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::operations::domain::entities::OperationRequest;
    use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
    use crate::modules::operations::infrastructure::persistence::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{OperationId, OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    struct VisibleSubjects {
        ids: BTreeSet<Uuid>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IOperationResourceAccess for VisibleSubjects {
        async fn subject_is_visible(
            &self,
            _organization_id: OrganizationId,
            subject: &OperationSubject,
            _evaluator: &ResourceAccessEvaluator,
        ) -> ApplicationResult<bool> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.ids.contains(&subject.id()))
        }
    }

    #[tokio::test]
    async fn restricted_lists_filter_candidates_in_feed_order_and_organization_roles_bypass_resolution(
    ) {
        let organization_id = OrganizationId::new();
        let repository = Arc::new(InMemoryOperationRepository::new());
        let now = Utc::now();
        let oldest = Uuid::now_v7();
        let middle = Uuid::now_v7();
        let newest = Uuid::now_v7();
        for (index, subject_id) in [oldest, middle, newest].into_iter().enumerate() {
            repository
                .enqueue(OperationRequest::new(
                    OperationId::from_uuid(subject_id),
                    organization_id,
                    OperationSubject::new("execution", subject_id).expect("subject"),
                    WorkflowIdentity::new("cloud.execution", "1").expect("workflow"),
                    serde_json::json!({}),
                    now + Duration::seconds(index as i64),
                ))
                .await
                .expect("enqueue");
        }
        let resolver = Arc::new(VisibleSubjects {
            ids: BTreeSet::from([oldest, newest]),
            calls: AtomicUsize::new(0),
        });
        let handler = ListOperationsHandler::new(repository, resolver.clone());
        let context = CqrsContext::new(ModuleRef::new());

        let restricted = handler
            .execute(
                ListOperations {
                    organization_id,
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Project {
                            project_id: ProjectId::new(),
                        },
                    ]),
                    limit: 2,
                },
                context.clone(),
            )
            .await
            .expect("framework")
            .expect("restricted list");
        assert_eq!(
            restricted
                .iter()
                .map(|record| record.request.subject.id())
                .collect::<Vec<_>>(),
            vec![newest, oldest]
        );
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 3);

        let organization_wide = handler
            .execute(
                ListOperations {
                    organization_id,
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                    limit: 2,
                },
                context,
            )
            .await
            .expect("framework")
            .expect("organization list");
        assert_eq!(
            organization_wide
                .iter()
                .map(|record| record.request.subject.id())
                .collect::<Vec<_>>(),
            vec![newest, middle]
        );
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn restricted_lists_continue_past_invisible_keyset_pages() {
        let organization_id = OrganizationId::new();
        let repository = Arc::new(InMemoryOperationRepository::new());
        let now = Utc::now();
        let visible_id = Uuid::now_v7();
        repository
            .enqueue(OperationRequest::new(
                OperationId::from_uuid(visible_id),
                organization_id,
                OperationSubject::new("execution", visible_id).expect("subject"),
                WorkflowIdentity::new("cloud.execution", "1").expect("workflow"),
                serde_json::json!({}),
                now,
            ))
            .await
            .expect("enqueue visible operation");
        for offset in 1..=201 {
            let subject_id = Uuid::now_v7();
            repository
                .enqueue(OperationRequest::new(
                    OperationId::from_uuid(subject_id),
                    organization_id,
                    OperationSubject::new("execution", subject_id).expect("subject"),
                    WorkflowIdentity::new("cloud.execution", "1").expect("workflow"),
                    serde_json::json!({}),
                    now + Duration::seconds(offset),
                ))
                .await
                .expect("enqueue invisible operation");
        }
        let resolver = Arc::new(VisibleSubjects {
            ids: BTreeSet::from([visible_id]),
            calls: AtomicUsize::new(0),
        });
        let handler = ListOperationsHandler::new(repository, resolver.clone());
        let records = handler
            .execute(
                ListOperations {
                    organization_id,
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Project {
                            project_id: ProjectId::new(),
                        },
                    ]),
                    limit: 1,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("restricted list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].request.subject.id(), visible_id);
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 202);
    }
}
