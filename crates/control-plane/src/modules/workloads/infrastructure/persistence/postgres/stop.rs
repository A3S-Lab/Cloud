use super::{queries, transitions};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, WorkloadId};
use crate::modules::workloads::domain::entities::{Workload, WorkloadDesiredState};
use crate::modules::workloads::domain::repositories::{
    RequestWorkloadStopBundle, WorkloadStopBundle,
};
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};

pub(super) async fn request(
    executor: &PostgresExecutor,
    request: RequestWorkloadStopBundle,
) -> Result<WorkloadStopBundle, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) =
                    idempotency_replay::<WorkloadStopBundle>(transaction, &request.idempotency)
                        .await?
                {
                    let mut response = replay.value;
                    response.replayed = true;
                    return Ok(response);
                }
                let current = queries::workload_in_transaction(
                    transaction,
                    request.workload.organization_id,
                    request.workload.id,
                    true,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if current.aggregate_version != request.expected_version {
                    return Err(RepositoryError::Conflict(format!(
                        "workload changed from expected version {} to {}",
                        request.expected_version, current.aggregate_version
                    ))
                    .into());
                }
                validate_request(&request, &current)?;
                if current != request.workload {
                    transitions::persist_workload(
                        transaction,
                        &request.workload,
                        current.aggregate_version,
                    )
                    .await?;
                }
                insert_operation(transaction, &request).await?;
                store_outbox(transaction, &request.event).await?;
                let response = WorkloadStopBundle {
                    workload: request.workload,
                    operation: request.operation,
                    replayed: false,
                };
                store_idempotency(transaction, &request.idempotency, &response).await?;
                Ok(response)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn complete(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    expected_version: u64,
    stopped_at: DateTime<Utc>,
) -> Result<Workload, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut workload = queries::workload_in_transaction(
                    transaction,
                    organization_id,
                    workload_id,
                    true,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if workload.aggregate_version != expected_version {
                    if workload.desired_state == WorkloadDesiredState::Stopped
                        && workload.active_revision_id.is_none()
                    {
                        return Ok(workload);
                    }
                    return Err(RepositoryError::Conflict(format!(
                        "workload changed from expected version {expected_version} to {}",
                        workload.aggregate_version
                    ))
                    .into());
                }
                let previous_version = workload.aggregate_version;
                workload.complete_stop(stopped_at).map_err(|error| {
                    RepositoryError::Conflict(format!(
                        "workload stop completion was rejected: {error}"
                    ))
                })?;
                if workload.aggregate_version != previous_version {
                    transitions::persist_workload(transaction, &workload, previous_version).await?;
                }
                Ok(workload)
            })
        })
        .await
        .map_err(transaction_error)
}

fn validate_request(
    request: &RequestWorkloadStopBundle,
    current: &Workload,
) -> Result<(), PostgresPersistenceError> {
    let mut expected = current.clone();
    expected
        .request_stop(request.workload.updated_at)
        .map_err(RepositoryError::Conflict)?;
    if expected != request.workload
        || request.operation.organization_id != request.workload.organization_id
        || request.operation.subject.kind() != "workload"
        || request.operation.subject.id() != request.workload.id.as_uuid()
        || request.operation.workflow.name() != "cloud.workload.stop"
        || request.operation.workflow.version() != "1"
        || request.operation.requested_at < request.workload.updated_at
        || request.event.organization_id != request.workload.organization_id.as_uuid()
        || request.event.aggregate_id != request.workload.id.as_uuid()
        || request.event.aggregate_version != request.workload.aggregate_version
    {
        return Err(RepositoryError::Conflict(
            "workload stop bundle is inconsistent with stored state".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_operation(
    transaction: &PostgresTransaction,
    request: &RequestWorkloadStopBundle,
) -> Result<(), PostgresPersistenceError> {
    let operation = &request.operation;
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (",
        )
        .bind(operation.id.as_uuid())
        .append(", ")
        .bind(operation.organization_id.as_uuid())
        .append(", ")
        .bind(operation.subject.kind())
        .append(", ")
        .bind(operation.subject.id())
        .append(", ")
        .bind(operation.workflow.name())
        .append(", ")
        .bind(operation.workflow.version())
        .append(", ")
        .bind(operation.input.clone())
        .append(", ")
        .bind(operation.requested_at)
        .append(")"),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("workload stop operation request", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "workload stop operation identity is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}
