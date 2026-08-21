use super::schema::OperationRequests;
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId, RepositoryError};
use a3s_orm::{insert_into, select_from, PostgresTransaction};
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

pub(super) async fn insert(
    transaction: &PostgresTransaction,
    operation: &OperationRequest,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        insert_into::<OperationRequests>()
            .value(OperationRequests::operation_id(), operation.id.as_uuid())
            .value(
                OperationRequests::organization_id(),
                operation.organization_id.as_uuid(),
            )
            .value(OperationRequests::subject_kind(), operation.subject.kind())
            .value(OperationRequests::subject_id(), operation.subject.id())
            .value(
                OperationRequests::workflow_name(),
                operation.workflow.name(),
            )
            .value(
                OperationRequests::workflow_version(),
                operation.workflow.version(),
            )
            .value(OperationRequests::input(), operation.input.clone())
            .value(OperationRequests::requested_at(), operation.requested_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("operation request", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => {
            Err(RepositoryError::Conflict("operation identity is already in use".into()).into())
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn find(
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
    Ok(OperationRequest::new(
        OperationId::from_uuid(id),
        OrganizationId::from_uuid(organization_id),
        OperationSubject::new(subject_kind, subject_id).map_err(RepositoryError::Storage)?,
        WorkflowIdentity::new(workflow_name, workflow_version).map_err(RepositoryError::Storage)?,
        input,
        requested_at,
    ))
}
