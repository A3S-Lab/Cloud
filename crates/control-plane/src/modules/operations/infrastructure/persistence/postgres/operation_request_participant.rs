//! Operations-owned same-transaction participant for `operation_requests`.
//!
//! Other contexts that must create or read an Operation request inside their
//! own Postgres transaction call these helpers instead of declaring a second
//! ORM mapping for the Operations table.

use super::schema::OperationRequests;
use crate::infrastructure::{
    PostgresPersistenceError, execute, fetch_optional, is_foreign_key_violation,
    is_unique_violation, require_one_row,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId, RepositoryError};
use a3s_orm::{PostgresTransaction, insert_into, select_from};
use chrono::{DateTime, Utc};
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

/// Insert one Operation request row inside a caller-owned transaction.
pub(crate) async fn insert(
    transaction: &PostgresTransaction,
    request: &OperationRequest,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
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
    match result {
        Ok(rows) => require_one_row("operation request", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => {
            Err(RepositoryError::Conflict("operation ID is already in use".into()).into())
        }
        Err(error) => Err(error),
    }
}

/// Load one Operation request by id inside a caller-owned transaction.
pub(crate) async fn find(
    transaction: &PostgresTransaction,
    operation_id: OperationId,
) -> Result<Option<OperationRequest>, PostgresPersistenceError> {
    fetch_optional::<OperationRequestRow, _>(
        transaction,
        select_from::<OperationRequests>()
            .select((
                OperationRequests::operation_id(),
                OperationRequests::organization_id(),
                OperationRequests::subject_kind(),
                OperationRequests::subject_id(),
                OperationRequests::workflow_name(),
                OperationRequests::workflow_version(),
                OperationRequests::input(),
                OperationRequests::requested_at(),
            ))
            .filter(OperationRequests::operation_id().eq(operation_id.as_uuid())),
    )
    .await?
    .map(decode)
    .transpose()
    .map_err(Into::into)
}

fn decode(row: OperationRequestRow) -> Result<OperationRequest, RepositoryError> {
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
