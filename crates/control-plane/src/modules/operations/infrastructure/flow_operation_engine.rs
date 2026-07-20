use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRequest, OperationStatus,
};
use crate::modules::operations::domain::services::{IOperationEngine, OperationEngineError};
use crate::modules::shared_kernel::domain::OperationId;
use a3s_flow::{FlowEngine, FlowError, WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

#[derive(Clone)]
pub struct FlowOperationEngine {
    engine: FlowEngine,
}

impl FlowOperationEngine {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl IOperationEngine for FlowOperationEngine {
    async fn ensure(
        &self,
        request: &OperationRequest,
    ) -> Result<OperationProjection, OperationEngineError> {
        let run_id = request.id.to_string();
        let spec = WorkflowSpec::rust_embedded(
            request.workflow.name(),
            request.workflow.version(),
            "a3s-cloud",
            "main",
        );
        self.engine
            .start_with_id(&run_id, spec, request.input.clone())
            .await
            .map_err(map_flow_error)?;
        snapshot_to_projection(
            self.engine
                .snapshot(&run_id)
                .await
                .map_err(map_flow_error)?,
        )
    }

    async fn projections(&self) -> Result<Vec<OperationProjection>, OperationEngineError> {
        self.engine
            .list_snapshots()
            .await
            .map_err(map_flow_error)?
            .into_iter()
            .map(snapshot_to_projection)
            .collect()
    }
}

fn snapshot_to_projection(
    snapshot: WorkflowRunSnapshot,
) -> Result<OperationProjection, OperationEngineError> {
    let operation_id = Uuid::parse_str(&snapshot.run_id)
        .map(OperationId::from_uuid)
        .map_err(|error| {
            OperationEngineError::Invalid(format!(
                "Flow run ID {:?} is not an operation UUID: {error}",
                snapshot.run_id
            ))
        })?;
    let status = match snapshot.status {
        WorkflowRunStatus::Pending => OperationStatus::Queued,
        WorkflowRunStatus::Running => OperationStatus::Running,
        WorkflowRunStatus::Suspended => OperationStatus::Suspended,
        WorkflowRunStatus::Completed
            if snapshot.output.as_ref().and_then(|output| {
                output
                    .get("operationStatus")
                    .and_then(|value| value.as_str())
            }) == Some("cancelled") =>
        {
            OperationStatus::Cancelled
        }
        WorkflowRunStatus::Completed => OperationStatus::Succeeded,
        WorkflowRunStatus::Failed => OperationStatus::Failed,
        WorkflowRunStatus::Cancelled => OperationStatus::Cancelled,
    };
    Ok(OperationProjection {
        operation_id,
        status,
        last_sequence: snapshot.last_sequence,
        output: snapshot.output,
        error: snapshot.error,
        updated_at: Utc::now(),
    })
}

fn map_flow_error(error: FlowError) -> OperationEngineError {
    match error {
        FlowError::RunConflict { .. } | FlowError::NonDeterministic { .. } => {
            OperationEngineError::Conflict(error.to_string())
        }
        FlowError::InvalidRunId(_)
        | FlowError::InvalidWorkflow(_)
        | FlowError::InvalidTransition(_)
        | FlowError::Serialization(_) => OperationEngineError::Invalid(error.to_string()),
        error => OperationEngineError::Unavailable(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn completed_cleanup_projects_the_operation_as_cancelled() {
        let operation_id = OperationId::new();
        let output = json!({
            "deploymentId": Uuid::now_v7(),
            "operationStatus": "cancelled",
        });
        let projection = snapshot_to_projection(completed_snapshot(operation_id, output.clone()))
            .expect("project completed cancellation");

        assert_eq!(projection.operation_id, operation_id);
        assert_eq!(projection.status, OperationStatus::Cancelled);
        assert_eq!(projection.output, Some(output));
    }

    #[test]
    fn ordinary_completed_flow_remains_succeeded() {
        let operation_id = OperationId::new();
        let projection = snapshot_to_projection(completed_snapshot(
            operation_id,
            json!({"deploymentId": Uuid::now_v7()}),
        ))
        .expect("project completed operation");

        assert_eq!(projection.status, OperationStatus::Succeeded);
    }

    fn completed_snapshot(
        operation_id: OperationId,
        output: serde_json::Value,
    ) -> WorkflowRunSnapshot {
        WorkflowRunSnapshot {
            run_id: operation_id.to_string(),
            spec: WorkflowSpec::rust_embedded("cloud.deployment", "2", "a3s-cloud", "main"),
            input: json!({}),
            status: WorkflowRunStatus::Completed,
            steps: BTreeMap::new(),
            waits: BTreeMap::new(),
            hooks: BTreeMap::new(),
            output: Some(output),
            error: None,
            last_sequence: 3,
        }
    }
}
