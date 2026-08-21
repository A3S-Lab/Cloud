mod schema;

use self::schema::{OperationProjections, OperationRequests};
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRecord, OperationRequest, OperationStatus,
};
use crate::modules::operations::domain::repositories::{
    IOperationRepository, OperationListCursor, OperationRefreshCursor,
};
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotentWrite, OperationId, OrganizationId, RepositoryError,
};
use a3s_orm::{
    insert_into, select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

type OperationRequestRow = (
    Uuid,
    Uuid,
    String,
    Uuid,
    String,
    String,
    serde_json::Value,
    DateTime<Utc>,
);

type OperationProjectionRow = (
    Uuid,
    String,
    u64,
    Option<serde_json::Value>,
    Option<String>,
    DateTime<Utc>,
);

#[derive(Clone)]
pub struct PostgresOperationRepository {
    executor: PostgresExecutor,
}

impl PostgresOperationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IOperationRepository for PostgresOperationRepository {
    async fn enqueue(
        &self,
        request: OperationRequest,
    ) -> Result<IdempotentWrite<OperationRequest>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_operation(transaction, request.id).await?;
                    if let Some(existing) =
                        find_request_in_transaction(transaction, request.id).await?
                    {
                        if !existing.has_same_definition(&request) {
                            return Err(RepositoryError::Conflict(
                                "operation ID was reused with a different request".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    let inserted = execute(
                        transaction,
                        insert_into::<OperationRequests>()
                            .value(OperationRequests::operation_id(), request.id.as_uuid())
                            .value(
                                OperationRequests::organization_id(),
                                request.organization_id.as_uuid(),
                            )
                            .value(OperationRequests::subject_kind(), request.subject.kind())
                            .value(OperationRequests::subject_id(), request.subject.id())
                            .value(OperationRequests::workflow_name(), request.workflow.name())
                            .value(
                                OperationRequests::workflow_version(),
                                request.workflow.version(),
                            )
                            .value(OperationRequests::input(), request.input.clone())
                            .value(OperationRequests::requested_at(), request.requested_at),
                    )
                    .await;
                    match inserted {
                        Ok(1) => Ok(IdempotentWrite {
                            value: request,
                            replayed: false,
                        }),
                        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
                            "enqueueing operation affected {rows} rows"
                        ))),
                        Err(error) if is_foreign_key_violation(&error) => {
                            Err(RepositoryError::NotFound.into())
                        }
                        Err(error) if is_unique_violation(&error) => Err(
                            RepositoryError::Conflict("operation ID is already in use".into())
                                .into(),
                        ),
                        Err(error) => Err(error),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn pending_starts(&self, limit: usize) -> Result<Vec<OperationRequest>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                operation_request_select()
                    .left_join::<OperationProjections>(
                        OperationProjections::operation_id()
                            .eq_column(OperationRequests::operation_id()),
                    )
                    .filter(OperationProjections::operation_id().is_null())
                    .order_by(OperationRequests::requested_at(), OrderDirection::Asc)
                    .order_by(OperationRequests::operation_id(), OrderDirection::Asc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_request)
            .collect()
    }

    async fn active_refreshes(
        &self,
        after: Option<OperationRefreshCursor>,
        limit: usize,
    ) -> Result<Vec<OperationRequest>, RepositoryError> {
        let mut query = operation_request_select()
            .inner_join::<OperationProjections>(
                OperationProjections::operation_id().eq_column(OperationRequests::operation_id()),
            )
            .filter(
                OperationProjections::status()
                    .ne("succeeded")
                    .and(OperationProjections::status().ne("failed"))
                    .and(OperationProjections::status().ne("cancelled")),
            );
        if let Some(after) = after {
            query = query.filter(
                OperationRequests::requested_at().gt(after.requested_at).or(
                    OperationRequests::requested_at()
                        .eq(after.requested_at)
                        .and(OperationRequests::operation_id().gt(after.operation_id.as_uuid())),
                ),
            );
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .order_by(OperationRequests::requested_at(), OrderDirection::Asc)
                    .order_by(OperationRequests::operation_id(), OrderDirection::Asc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_request)
            .collect()
    }

    async fn find_request(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationRequest>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(request_query(operation_id))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_request)
            .transpose()
    }

    async fn upsert_projection(
        &self,
        mut projection: OperationProjection,
    ) -> Result<bool, RepositoryError> {
        projection.updated_at = canonical_timestamp(projection.updated_at);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_operation(transaction, projection.operation_id).await?;
                    if let Some(existing) =
                        find_projection_in_transaction(transaction, projection.operation_id).await?
                    {
                        if existing.last_sequence > projection.last_sequence {
                            return Ok(false);
                        }
                        if existing.last_sequence == projection.last_sequence {
                            if existing.status != projection.status
                                || existing.output != projection.output
                                || existing.error != projection.error
                            {
                                return Err(PostgresPersistenceError::Invariant(
                                    "operation projection changed without advancing its sequence"
                                        .into(),
                                ));
                            }
                            return Ok(false);
                        }
                    }
                    let written = execute(
                        transaction,
                        insert_into::<OperationProjections>()
                            .value(
                                OperationProjections::operation_id(),
                                projection.operation_id.as_uuid(),
                            )
                            .value(OperationProjections::status(), projection.status.as_str())
                            .value(
                                OperationProjections::last_sequence(),
                                projection.last_sequence,
                            )
                            .value(OperationProjections::output(), projection.output.clone())
                            .value(OperationProjections::error(), projection.error.clone())
                            .value(OperationProjections::updated_at(), projection.updated_at)
                            .on_conflict(OperationProjections::operation_id())
                            .do_update_from_excluded(OperationProjections::status())
                            .do_update_from_excluded(OperationProjections::last_sequence())
                            .do_update_from_excluded(OperationProjections::output())
                            .do_update_from_excluded(OperationProjections::error())
                            .do_update_from_excluded(OperationProjections::updated_at()),
                    )
                    .await;
                    match written {
                        Ok(1) => Ok(true),
                        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
                            "projecting operation affected {rows} rows"
                        ))),
                        Err(error) if is_foreign_key_violation(&error) => {
                            Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => Err(error),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_projection(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationProjection>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(projection_query(operation_id))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_projection)
            .transpose()
    }

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        after: Option<OperationListCursor>,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, RepositoryError> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        let mut requests_query = operation_request_select()
            .filter(OperationRequests::organization_id().eq(organization_id.as_uuid()));
        let mut projections_query = operation_projection_select()
            .inner_join::<OperationRequests>(
                OperationRequests::operation_id().eq_column(OperationProjections::operation_id()),
            )
            .filter(OperationRequests::organization_id().eq(organization_id.as_uuid()));
        if let Some(after) = after {
            let after_filter = OperationRequests::requested_at().lt(after.requested_at).or(
                OperationRequests::requested_at()
                    .eq(after.requested_at)
                    .and(OperationRequests::operation_id().gt(after.operation_id.as_uuid())),
            );
            requests_query = requests_query.filter(after_filter.clone());
            projections_query = projections_query.filter(after_filter);
        }
        let requests = database
            .fetch_all_as(
                requests_query
                    .order_by(OperationRequests::requested_at(), OrderDirection::Desc)
                    .order_by(OperationRequests::operation_id(), OrderDirection::Asc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_request)
            .collect::<Result<Vec<_>, _>>()?;
        let projections = database
            .fetch_all_as(
                projections_query
                    .order_by(OperationRequests::requested_at(), OrderDirection::Desc)
                    .order_by(OperationRequests::operation_id(), OrderDirection::Asc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_projection)
            .map(|projection| projection.map(|value| (value.operation_id, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(requests
            .into_iter()
            .map(|request| OperationRecord {
                projection: projections.get(&request.id).cloned(),
                request,
            })
            .collect())
    }
}

async fn lock_operation(
    transaction: &a3s_orm::PostgresTransaction,
    operation_id: OperationId,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock("cloud.operation", &operation_id.to_string())
        .await?;
    Ok(())
}

async fn find_request_in_transaction(
    transaction: &a3s_orm::PostgresTransaction,
    operation_id: OperationId,
) -> Result<Option<OperationRequest>, PostgresPersistenceError> {
    fetch_optional::<OperationRequestRow, _>(transaction, request_query(operation_id))
        .await?
        .map(decode_request)
        .transpose()
        .map_err(Into::into)
}

async fn find_projection_in_transaction(
    transaction: &a3s_orm::PostgresTransaction,
    operation_id: OperationId,
) -> Result<Option<OperationProjection>, PostgresPersistenceError> {
    fetch_optional::<OperationProjectionRow, _>(transaction, projection_query(operation_id))
        .await?
        .map(decode_projection)
        .transpose()
        .map_err(Into::into)
}

fn request_query(
    operation_id: OperationId,
) -> a3s_orm::query::SelectQuery<OperationRequests, OperationRequestRow> {
    operation_request_select().filter(OperationRequests::operation_id().eq(operation_id.as_uuid()))
}

fn projection_query(
    operation_id: OperationId,
) -> a3s_orm::query::SelectQuery<OperationProjections, OperationProjectionRow> {
    operation_projection_select()
        .filter(OperationProjections::operation_id().eq(operation_id.as_uuid()))
}

fn operation_request_select() -> a3s_orm::query::SelectQuery<OperationRequests, OperationRequestRow>
{
    select_from::<OperationRequests>().select((
        OperationRequests::operation_id(),
        OperationRequests::organization_id(),
        OperationRequests::subject_kind(),
        OperationRequests::subject_id(),
        OperationRequests::workflow_name(),
        OperationRequests::workflow_version(),
        OperationRequests::input(),
        OperationRequests::requested_at(),
    ))
}

fn operation_projection_select(
) -> a3s_orm::query::SelectQuery<OperationProjections, OperationProjectionRow> {
    select_from::<OperationProjections>().select((
        OperationProjections::operation_id(),
        OperationProjections::status(),
        OperationProjections::last_sequence(),
        OperationProjections::output(),
        OperationProjections::error(),
        OperationProjections::updated_at(),
    ))
}

fn decode_request(row: OperationRequestRow) -> Result<OperationRequest, RepositoryError> {
    let (
        id,
        organization_id,
        subject_kind,
        subject_id,
        workflow_name,
        workflow_version,
        input,
        requested_at,
    ) = row;
    let subject = OperationSubject::new(subject_kind, subject_id).map_err(|error| {
        RepositoryError::Storage(format!("stored operation subject is invalid: {error}"))
    })?;
    let workflow = WorkflowIdentity::new(workflow_name, workflow_version).map_err(|error| {
        RepositoryError::Storage(format!("stored workflow identity is invalid: {error}"))
    })?;
    Ok(OperationRequest::new(
        OperationId::from_uuid(id),
        OrganizationId::from_uuid(organization_id),
        subject,
        workflow,
        input,
        requested_at,
    ))
}

fn decode_projection(row: OperationProjectionRow) -> Result<OperationProjection, RepositoryError> {
    let (operation_id, status, last_sequence, output, error, updated_at) = row;
    let status = OperationStatus::parse(&status).map_err(|error| {
        RepositoryError::Storage(format!("stored operation projection is invalid: {error}"))
    })?;
    Ok(OperationProjection {
        operation_id: OperationId::from_uuid(operation_id),
        status,
        last_sequence,
        output,
        error,
        updated_at,
    })
}
