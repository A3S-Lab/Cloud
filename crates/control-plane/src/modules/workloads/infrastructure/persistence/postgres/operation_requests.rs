use super::schema::OperationRequests;
use crate::infrastructure::{
    execute, is_foreign_key_violation, is_unique_violation, require_one_row,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{insert_into, PostgresTransaction};

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
