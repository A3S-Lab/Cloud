use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRecord, OperationRequest, OperationStatus,
};
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
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
                    if let Some(existing) = find_request_in_transaction(transaction, request.id).await?
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
                        sql_query::<()>(
                            "insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (",
                        )
                        .bind(request.id.as_uuid())
                        .append(", ")
                        .bind(request.organization_id.as_uuid())
                        .append(", ")
                        .bind(request.subject.kind())
                        .append(", ")
                        .bind(request.subject.id())
                        .append(", ")
                        .bind(request.workflow.name())
                        .append(", ")
                        .bind(request.workflow.version())
                        .append(", ")
                        .bind(request.input.clone())
                        .append(", ")
                        .bind(request.requested_at)
                        .append(")"),
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
                        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
                            "operation ID is already in use".into(),
                        )
                        .into()),
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
                sql_query::<OperationRequestRow>(
                    "select r.operation_id, r.organization_id, r.subject_kind, r.subject_id, r.workflow_name, r.workflow_version, r.input, r.requested_at from operation_requests r left join operation_projections p on p.operation_id = r.operation_id where p.operation_id is null or p.status not in ('succeeded', 'failed', 'cancelled') order by r.requested_at asc, r.operation_id asc limit ",
                )
                .bind(limit.max(1)),
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
        projection: OperationProjection,
    ) -> Result<(), RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_operation(transaction, projection.operation_id).await?;
                    if let Some(existing) =
                        find_projection_in_transaction(transaction, projection.operation_id).await?
                    {
                        if existing.last_sequence > projection.last_sequence {
                            return Ok(());
                        }
                        if existing.last_sequence == projection.last_sequence
                            && (existing.status != projection.status
                                || existing.output != projection.output
                                || existing.error != projection.error)
                        {
                            return Err(PostgresPersistenceError::Invariant(
                                "operation projection changed without advancing its sequence".into(),
                            ));
                        }
                    }
                    let written = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into operation_projections (operation_id, status, last_sequence, output, error, updated_at) values (",
                        )
                        .bind(projection.operation_id.as_uuid())
                        .append(", ")
                        .bind(projection.status.as_str())
                        .append(", ")
                        .bind(projection.last_sequence)
                        .append(", ")
                        .bind(projection.output.clone())
                        .append(", ")
                        .bind(projection.error.as_deref())
                        .append(", ")
                        .bind(projection.updated_at)
                        .append(") on conflict (operation_id) do update set status = excluded.status, last_sequence = excluded.last_sequence, output = excluded.output, error = excluded.error, updated_at = excluded.updated_at"),
                    )
                    .await;
                    match written {
                        Ok(1) => Ok(()),
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

    async fn list(
        &self,
        organization_id: OrganizationId,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, RepositoryError> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        let requests = database
            .fetch_all_as(
                sql_query::<OperationRequestRow>(
                    "select operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at from operation_requests where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" order by requested_at desc, operation_id asc limit ")
                .bind(limit.max(1)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_request)
            .collect::<Result<Vec<_>, _>>()?;
        let projections = database
            .fetch_all_as(
                sql_query::<OperationProjectionRow>(
                    "select p.operation_id, p.status, p.last_sequence, p.output, p.error, p.updated_at from operation_projections p join operation_requests r on r.operation_id = p.operation_id where r.organization_id = ",
                )
                .bind(organization_id.as_uuid()),
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
    let locked = fetch_optional::<i32, _>(
        transaction,
        sql_query::<i32>("select 1 from (select pg_advisory_xact_lock(hashtext(")
            .bind("cloud.operation")
            .append("), hashtext(")
            .bind(operation_id.to_string())
            .append("))) as locked"),
    )
    .await?;
    if locked == Some(1) {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Invariant(
            "operation advisory lock did not return a row".into(),
        ))
    }
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

fn request_query(operation_id: OperationId) -> a3s_orm::SqlQuery<OperationRequestRow> {
    sql_query::<OperationRequestRow>(
        "select operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at from operation_requests where operation_id = ",
    )
    .bind(operation_id.as_uuid())
}

fn projection_query(operation_id: OperationId) -> a3s_orm::SqlQuery<OperationProjectionRow> {
    sql_query::<OperationProjectionRow>(
        "select operation_id, status, last_sequence, output, error, updated_at from operation_projections where operation_id = ",
    )
    .bind(operation_id.as_uuid())
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
