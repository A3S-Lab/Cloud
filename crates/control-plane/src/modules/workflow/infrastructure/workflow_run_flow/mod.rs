mod composite;
mod connector;
mod connector_response;
mod coordinator;
mod diagnostics;
mod execution;
mod projection;
mod projection_authority;
mod readers;
mod variables;
mod workflow;
mod workflow_application_variables;

#[cfg(test)]
mod connector_response_tests;

pub use coordinator::FlowWorkflowRunCoordinator;
pub use diagnostics::WorkflowRunDiagnosticsReader;
pub use projection::project_workflow_run_record;
pub use readers::{WorkflowRunHistoryReader, WorkflowRunVariableReader};

use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::workflow::domain::{
    descriptor_failure_output, ResolvedWorkflowRunStep, WorkflowRunInput,
    WorkflowStepDefaultOutputEvidence, WorkflowStepFailureOutput, WorkflowStepKind,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_STEP_NAME: &str = "workflow_run_local";

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    [
        WORKFLOW_RUN_STEP_NAME,
        connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME,
    ]
    .into_iter()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V2,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V3,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V4,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V5,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V6,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V7,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V8,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V9,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V10,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V11,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V12,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V13,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V14,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V15,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V16,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V17,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V18,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V19,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V20,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V21,
        ),
    ]
    .into_iter()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowLocalStepInput {
    runtime_contract_revision: String,
    #[serde(default, skip_serializing_if = "is_false")]
    typed_projection_authoritative: bool,
    step: ResolvedWorkflowRunStep,
    workflow_input: serde_json::Value,
    effective_input: serde_json::Value,
    dependencies: BTreeMap<String, serde_json::Value>,
    steps: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    routed_failure: Option<WorkflowStepFailureOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composite_region_result:
        Option<crate::modules::workflow::domain::WorkflowCompositeRegionResult>,
}

const fn is_false(value: &bool) -> bool {
    !*value
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_region_result:
        Option<crate::modules::workflow::domain::WorkflowCompositeRegionResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output_evidence: Option<WorkflowStepDefaultOutputEvidence>,
}

impl WorkflowLocalStepResult {
    pub fn validate(&self, expected: &ResolvedWorkflowRunStep) -> Result<(), String> {
        if self.step_id != expected.plan.id || self.kind != expected.plan.kind {
            return Err("Workflow local step result identity drifted".into());
        }
        let branch_failure = self.kind == WorkflowStepKind::Branch
            && self.selected_handle.as_deref().is_some_and(|selected| {
                expected
                    .plan
                    .failure
                    .as_ref()
                    .and_then(|failure| descriptor_failure_output(failure).ok())
                    .is_some_and(|output| output.name == selected)
            });
        let routed_failure = self.selected_handle.is_some()
            && (self.kind != WorkflowStepKind::Branch || branch_failure);
        if let Some(evidence) = self.default_output_evidence.as_ref() {
            if routed_failure || self.kind != WorkflowStepKind::Execution {
                return Err("Workflow default-output result has invalid control flow".into());
            }
            evidence.validate(expected)?;
            let material = expected
                .policy
                .as_ref()
                .and_then(|policy| policy.default_output.as_ref())
                .ok_or_else(|| {
                    "Workflow default-output result lost its immutable material".to_owned()
                })?;
            if self.output != material.value || self.output_digest != material.digest {
                return Err("Workflow default-output result drifted from its exact policy".into());
            }
            execution::validate_data_schema(
                &expected.output_schema,
                &self.output,
                "Workflow default output",
            )?;
        } else if routed_failure {
            let failure = serde_json::from_value::<WorkflowStepFailureOutput>(self.output.clone())
                .map_err(|error| format!("Workflow step failure output is invalid: {error}"))?;
            failure.validate(expected)?;
            let expected_handle =
                descriptor_failure_output(expected.plan.failure.as_ref().ok_or_else(|| {
                    "Workflow failure result lost its immutable contract".to_owned()
                })?)?
                .name
                .as_str();
            if self.selected_handle.as_deref() != Some(expected_handle) {
                return Err("Workflow failure selected the wrong error handle".into());
            }
        } else {
            execution::validate_data_schema(
                &expected.output_schema,
                &self.output,
                "Workflow step output",
            )?;
        }
        let digest = execution::value_digest(&self.output, "Workflow step output")?;
        if digest != self.output_digest {
            return Err("Workflow local step output digest does not match".into());
        }
        if self.kind == WorkflowStepKind::Branch && !routed_failure {
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
        } else if self.selected_handle.is_some() && !routed_failure {
            return Err("non-branch Workflow step selected a handle".into());
        }
        match (self.kind, self.composite_region_result.as_ref()) {
            (WorkflowStepKind::Subworkflow, Some(region))
                if region.region_step_id == self.step_id
                    && region.output == self.output
                    && region.output_digest == self.output_digest => {}
            (WorkflowStepKind::Subworkflow, None) if routed_failure => {}
            (WorkflowStepKind::Subworkflow, _) => {
                return Err("Workflow composite step result lost its region evidence".into())
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err("non-composite Workflow step retained region evidence".into())
            }
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct WorkflowRunFlowRuntime {
    connector_responses:
        Option<std::sync::Arc<dyn crate::modules::connectors::IConnectorResponseObjectPort>>,
}

impl WorkflowRunFlowRuntime {
    pub fn with_connector_responses(
        connector_responses: std::sync::Arc<
            dyn crate::modules::connectors::IConnectorResponseObjectPort,
        >,
    ) -> Self {
        Self {
            connector_responses: Some(connector_responses),
        }
    }
}

impl std::fmt::Debug for WorkflowRunFlowRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowRunFlowRuntime")
            .field(
                "connector_responses_configured",
                &self.connector_responses.is_some(),
            )
            .finish()
    }
}

#[async_trait::async_trait]
impl FlowRuntime for WorkflowRunFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        workflow::run_workflow(invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        match invocation.step_name.as_str() {
            WORKFLOW_RUN_STEP_NAME => {
                let input: WorkflowLocalStepInput = invocation.input_as()?;
                if !matches!(
                    input.runtime_contract_revision.as_str(),
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
                ) {
                    return Err(FlowError::Runtime(
                        "WorkflowRun step runtime contract revision is unsupported".into(),
                    ));
                }
                let result = execution::execute_local_step(&input).map_err(FlowError::Runtime)?;
                serde_json::to_value(result).map_err(FlowError::from)
            }
            connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME => {
                let input: connector_response::WorkflowConnectorResponseStepInput =
                    invocation.input_as()?;
                let responses = self.connector_responses.as_deref().ok_or_else(|| {
                    FlowError::Runtime(
                        "Workflow Connector response consumption is not configured".into(),
                    )
                })?;
                let result = connector_response::consume_response(&input, responses)
                    .await
                    .map_err(FlowError::Runtime)?;
                serde_json::to_value(result).map_err(FlowError::from)
            }
            _ => Err(FlowError::Runtime(format!(
                "WorkflowRun runtime cannot execute step name {:?}",
                invocation.step_name
            ))),
        }
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
#[path = "runtime_tests.rs"]
mod tests;

#[cfg(test)]
mod connector_tests;
