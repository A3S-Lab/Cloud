use crate::modules::operations::domain::entities::OperationRecord;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workflow::application::{workflow_run_operation, WorkflowRunView};
use crate::modules::workflow::domain::WorkflowRun;

pub(super) async fn load(
    run: WorkflowRun,
    operations: &dyn IOperationRepository,
) -> Result<WorkflowRunView, RepositoryError> {
    let request = operations
        .find_request(run.operation_id)
        .await?
        .ok_or_else(|| {
            RepositoryError::Storage("WorkflowRun Operation request is missing".into())
        })?;
    let expected = workflow_run_operation(&run).map_err(RepositoryError::Storage)?;
    if !request.has_same_definition(&expected) {
        return Err(RepositoryError::Storage(
            "WorkflowRun Operation request changed identity".into(),
        ));
    }
    let projection = operations.find_projection(run.operation_id).await?;
    Ok(WorkflowRunView {
        run,
        operation: OperationRecord {
            request,
            projection,
        },
    })
}
