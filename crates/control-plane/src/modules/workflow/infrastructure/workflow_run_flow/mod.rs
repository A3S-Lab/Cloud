mod coordinator;
mod execution;
mod projection;
mod workflow;

pub use coordinator::FlowWorkflowRunCoordinator;
pub use projection::{project_workflow_run_record, WorkflowRunHistoryReader};

use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::workflow::domain::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_STEP_NAME: &str = "workflow_run_local";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowLocalStepInput {
    runtime_contract_revision: String,
    step: ResolvedWorkflowRunStep,
    workflow_input: serde_json::Value,
    effective_input: serde_json::Value,
    dependencies: BTreeMap<String, serde_json::Value>,
    steps: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowLocalStepResult {
    pub step_id: String,
    pub kind: WorkflowStepKind,
    pub output: serde_json::Value,
    pub output_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_handle: Option<String>,
}

impl WorkflowLocalStepResult {
    pub fn validate(&self, expected: &ResolvedWorkflowRunStep) -> Result<(), String> {
        if self.step_id != expected.plan.id || self.kind != expected.plan.kind {
            return Err("Workflow local step result identity drifted".into());
        }
        execution::validate_data_schema(
            &expected.output_schema,
            &self.output,
            "Workflow step output",
        )?;
        let digest = execution::value_digest(&self.output, "Workflow step output")?;
        if digest != self.output_digest {
            return Err("Workflow local step output digest does not match".into());
        }
        if self.kind == WorkflowStepKind::Branch {
            let selected = self
                .selected_handle
                .as_deref()
                .ok_or_else(|| "Workflow branch result did not select a handle".to_owned())?;
            if !expected
                .configuration
                .routes
                .iter()
                .any(|route| route.handle == selected)
            {
                return Err(format!(
                    "Workflow branch selected undeclared handle {selected:?}"
                ));
            }
        } else if self.selected_handle.is_some() {
            return Err("non-branch Workflow step selected a handle".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowRunFlowRuntime;

#[async_trait::async_trait]
impl FlowRuntime for WorkflowRunFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        workflow::run_workflow(invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        if invocation.step_name != WORKFLOW_RUN_STEP_NAME {
            return Err(FlowError::Runtime(format!(
                "WorkflowRun runtime cannot execute step name {:?}",
                invocation.step_name
            )));
        }
        let input: WorkflowLocalStepInput = invocation.input_as()?;
        if input.runtime_contract_revision != WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION {
            return Err(FlowError::Runtime(
                "WorkflowRun step runtime contract revision is unsupported".into(),
            ));
        }
        let result = execution::execute_local_step(&input).map_err(FlowError::Runtime)?;
        serde_json::to_value(result).map_err(FlowError::from)
    }
}

fn decode_input(value: serde_json::Value) -> Result<WorkflowRunInput, FlowError> {
    let input: WorkflowRunInput = serde_json::from_value(value)?;
    input.validate().map_err(|error| {
        FlowError::InvalidWorkflow(format!("invalid WorkflowRun input: {error}"))
    })?;
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::{
        flow_step_id, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    };
    use crate::modules::workflow::test_support::workflow_run_input;
    use a3s_flow::{FlowEngine, WorkflowRunStatus, WorkflowSpec};
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn flow_engine_executes_and_idempotently_replays_the_minimal_workflow_run(
    ) -> Result<(), FlowError> {
        let mut input = workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let spec = WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION,
            "cloud",
            "workflow_run",
        );
        let encoded = serde_json::to_value(&input)?;
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(run_id.clone(), spec.clone(), encoded.clone())
            .await?;
        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(json!("HIGH T-42")));
        assert!(snapshot.steps.contains_key(&flow_step_id("high")));
        assert!(!snapshot.steps.contains_key(&flow_step_id("normal")));

        let history_length = engine.history(&run_id).await?.len();
        engine
            .start_with_id(run_id.clone(), spec.clone(), encoded)
            .await?;
        assert_eq!(engine.history(&run_id).await?.len(), history_length);

        let mut drifted = input;
        drifted.goal_input["ticketId"] = json!("T-99");
        assert!(engine
            .start_with_id(run_id, spec, serde_json::to_value(drifted)?)
            .await
            .is_err());
        Ok(())
    }
}
