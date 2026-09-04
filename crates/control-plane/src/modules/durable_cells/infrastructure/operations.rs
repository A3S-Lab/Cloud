use crate::modules::durable_cells::application::{
    DurableCellOperationLookupRequest, DurableCellOperationProjection,
    DurableCellOperationRequestProjection, DurableCellOperationSnapshot,
    DurableCellOperationStatus, IDurableCellOperationPort,
};
use crate::modules::operations::{IOperationRepository, OperationStatus};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from the Operations owner to the Durable Cells
/// consumer-owned read port. Operations retains request/projection storage and
/// lifecycle interpretation; Durable Cells receives only a validated snapshot.
#[derive(Clone)]
pub(crate) struct OperationsDurableCellOperationAdapter {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsDurableCellOperationAdapter {
    pub(crate) fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IDurableCellOperationPort for OperationsDurableCellOperationAdapter {
    async fn load_exact(
        &self,
        request: &DurableCellOperationLookupRequest,
    ) -> ApplicationResult<DurableCellOperationSnapshot> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let stored = self
            .operations
            .find_request(request.operation_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound(
                    "Durable Cell continuation Operation request not found".into(),
                )
            })?;
        let operation_request = DurableCellOperationRequestProjection {
            operation_id: stored.id,
            organization_id: stored.organization_id,
            subject_kind: stored.subject.kind().to_owned(),
            subject_id: stored.subject.id(),
            workflow_name: stored.workflow.name().to_owned(),
            workflow_version: stored.workflow.version().to_owned(),
            input: stored.input,
            requested_at: stored.requested_at,
        };
        operation_request
            .validate_against(request)
            .map_err(ApplicationError::Conflict)?;

        let projection = self
            .operations
            .find_projection(request.operation_id)
            .await?
            .map(project_projection);
        let snapshot = DurableCellOperationSnapshot {
            request: operation_request,
            projection,
        };
        snapshot
            .validate_against(request)
            .map_err(ApplicationError::Internal)?;
        Ok(snapshot)
    }
}

fn project_projection(
    projection: crate::modules::operations::OperationProjection,
) -> DurableCellOperationProjection {
    let status = match projection.status {
        OperationStatus::Queued => DurableCellOperationStatus::Queued,
        OperationStatus::Running => DurableCellOperationStatus::Running,
        OperationStatus::Suspended => DurableCellOperationStatus::Suspended,
        OperationStatus::Cancelling => DurableCellOperationStatus::Cancelling,
        OperationStatus::Succeeded => DurableCellOperationStatus::Succeeded,
        OperationStatus::Failed => DurableCellOperationStatus::Failed,
        OperationStatus::Cancelled => DurableCellOperationStatus::Cancelled,
    };
    DurableCellOperationProjection {
        operation_id: projection.operation_id,
        status,
        last_sequence: projection.last_sequence,
        output: projection.output,
        error: projection.error,
        updated_at: projection.updated_at,
    }
}
