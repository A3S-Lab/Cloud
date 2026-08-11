use crate::modules::shared_kernel::domain::{PlanRevisionId, WorkflowRunId};
use crate::modules::workflow::domain::{
    WorkflowDataSchema, WorkflowPlanStep, WorkflowStepConfiguration, WorkflowStepKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WORKFLOW_LOCAL_STEP_NAME: &str = "workflow_local_step";
pub const WORKFLOW_LOCAL_STEP_INPUT_SCHEMA: &str = "cloud.workflow.local-step-input.v1";
pub const WORKFLOW_LOCAL_STEP_RESULT_SCHEMA: &str = "cloud.workflow.local-step-result.v1";
pub const WORKFLOW_RUN_OUTPUT_SCHEMA: &str = "cloud.workflow.run-output.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowLocalStepInput {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: String,
    pub step: WorkflowPlanStep,
    pub configuration: WorkflowStepConfiguration,
    pub input_schema: WorkflowDataSchema,
    pub output_schema: WorkflowDataSchema,
    pub workflow_input: Value,
    pub dependencies: BTreeMap<String, Value>,
}

impl WorkflowLocalStepInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_LOCAL_STEP_INPUT_SCHEMA
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.plan_digest.len() != 71
            || !self.plan_digest.starts_with("sha256:")
            || self.configuration.step_kind != self.step.kind
        {
            return Err("Workflow local step authority is invalid".into());
        }
        if !matches!(
            self.step.kind,
            WorkflowStepKind::Input
                | WorkflowStepKind::Transform
                | WorkflowStepKind::Branch
                | WorkflowStepKind::Output
        ) {
            return Err(format!(
                "Workflow local step kind {} is unavailable",
                self.step.kind.as_str()
            ));
        }
        self.configuration.validate()?;
        self.input_schema.validate()?;
        self.output_schema.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowLocalStepResult {
    pub schema: String,
    pub step_id: String,
    pub kind: WorkflowStepKind,
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

impl WorkflowLocalStepResult {
    pub fn validate(
        &self,
        step: &WorkflowPlanStep,
        output_schema: &WorkflowDataSchema,
    ) -> Result<(), String> {
        if self.schema != WORKFLOW_LOCAL_STEP_RESULT_SCHEMA
            || self.step_id != step.id
            || self.kind != step.kind
        {
            return Err("Workflow local step result changed identity".into());
        }
        match step.kind {
            WorkflowStepKind::Branch if self.route.is_none() => {
                return Err("Workflow branch result omitted its route".into())
            }
            WorkflowStepKind::Branch => {}
            _ if self.route.is_some() => {
                return Err("Only a Workflow branch may select a route".into())
            }
            _ => {}
        }
        output_schema.validate_value(&self.output)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRunOutput {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: String,
    pub output: Value,
}
