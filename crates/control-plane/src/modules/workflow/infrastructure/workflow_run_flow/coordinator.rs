use super::project_workflow_run_record;
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowRunCoordinationError, WorkflowRunRecord, WorkflowRunStatus,
};
use a3s_flow::{CancellationRequest, FlowEngine, FlowError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct FlowWorkflowRunCoordinator {
    engine: FlowEngine,
}

impl FlowWorkflowRunCoordinator {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl IWorkflowRunCoordinator for FlowWorkflowRunCoordinator {
    async fn reconcile(
        &self,
        record: &WorkflowRunRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowRunCoordinationError> {
        let mut snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => {
                return Err(WorkflowRunCoordinationError::Deferred(
                    "the correlated Operation has not started A3S Flow".into(),
                ))
            }
            Err(error) => return Err(unavailable(error)),
        };
        if !snapshot.status.is_terminal() && record.run.status == WorkflowRunStatus::Cancelling {
            snapshot = self
                .engine
                .request_cancellation(
                    &record.run.flow_run_id,
                    CancellationRequest::new(record.run.cancellation_reason.clone()),
                )
                .await
                .map_err(unavailable)?;
        } else if !snapshot.status.is_terminal() && now >= record.run.execution_input.deadline_at {
            self.engine
                .terminate_for_timeout(
                    &record.run.flow_run_id,
                    record.run.execution_input.deadline_at,
                    Some("WorkflowRun exceeded its immutable deadline".into()),
                )
                .await
                .map_err(unavailable)?;
            snapshot = self
                .engine
                .snapshot(&record.run.flow_run_id)
                .await
                .map_err(unavailable)?;
        }
        let history = self
            .engine
            .history(&record.run.flow_run_id)
            .await
            .map_err(unavailable)?;
        match project_workflow_run_record(record, &snapshot, &history) {
            Ok(projected) => Ok(projected),
            Err(error) => project_drift(record, &snapshot, &history, error).map(Some),
        }
    }
}

fn project_drift(
    record: &WorkflowRunRecord,
    snapshot: &a3s_flow::WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
    error: String,
) -> Result<WorkflowRunRecord, WorkflowRunCoordinationError> {
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .unwrap_or_else(Utc::now);
    let started_at = history.iter().find_map(|event| {
        matches!(event.event, a3s_flow::FlowEvent::RunStarted).then_some(event.timestamp)
    });
    let build_id = snapshot
        .spec
        .runtime_build_id
        .as_ref()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "unpinned-flow-runtime".into());
    let mut projected = record.clone();
    projected
        .run
        .project_flow(crate::modules::workflow::domain::WorkflowRunFlowState {
            status: WorkflowRunStatus::Failed,
            flow_runtime_build_id: build_id,
            last_flow_sequence: snapshot.last_sequence.max(1),
            output: None,
            error: Some(format!("WorkflowRun replay drift: {error}")),
            started_at,
            finished_at: Some(observed_at),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    for step in &mut projected.steps {
        if step.status.is_terminal() {
            continue;
        }
        step.project_flow(crate::modules::workflow::domain::WorkflowStepFlowState {
            status: crate::modules::workflow::domain::WorkflowStepProjectionStatus::Cancelled,
            attempt_generation: step.attempt_generation,
            selected_handle: None,
            result: None,
            error: None,
            last_flow_sequence: snapshot.last_sequence.max(1),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    }
    projected
        .validate()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    Ok(projected)
}

fn unavailable(error: FlowError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(error.to_string())
}
