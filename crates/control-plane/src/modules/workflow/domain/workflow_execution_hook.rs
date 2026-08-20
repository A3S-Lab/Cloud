use super::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind, WORKFLOW_RUN_INPUT_MAX_BYTES,
    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, EnvironmentId, ExecutionId,
    ExecutionTemplateId, ExecutionTemplateRevisionId, OperationId, OrganizationId, PlanRevisionId,
    ProjectId, Sha256Digest, WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_EXECUTION_HOOK_SCHEMA: &str = "cloud.workflow.execution-hook.v1";
pub const WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA: &str =
    "cloud.workflow.execution-child-reference.v1";
pub const WORKFLOW_EXECUTION_RESUME_SCHEMA: &str = "cloud.workflow.execution-resume.v1";
pub const WORKFLOW_EXECUTION_RESULT_SCHEMA: &str = "cloud.workflow.execution-result.v1";
pub const WORKFLOW_EXECUTION_STEP_ATTEMPT: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowExecutionHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub configuration_digest: Sha256Digest,
    pub execution_template_id: ExecutionTemplateId,
    pub execution_template_revision_id: ExecutionTemplateRevisionId,
    pub execution_template_digest: Sha256Digest,
    pub capability: String,
    pub effective_input: serde_json::Value,
    pub effective_input_digest: Sha256Digest,
}

impl WorkflowExecutionHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        effective_input: serde_json::Value,
    ) -> Result<Self, String> {
        if step.plan.kind != WorkflowStepKind::Execution {
            return Err("Workflow execution hook requires an Execution step".into());
        }
        let environment_id = input.plan.environment_id.ok_or_else(|| {
            "Workflow execution step requires one exact target environment".to_owned()
        })?;
        let capability = step
            .plan
            .capability
            .as_ref()
            .ok_or_else(|| "Workflow execution step lost its ExecutionTemplate".to_owned())?;
        capability.validate()?;
        if capability.capability_type != super::CapabilityType::ExecutionTemplate {
            return Err("Workflow execution step has the wrong capability type".into());
        }
        let revision_id = uuid::Uuid::parse_str(&capability.revision)
            .map(ExecutionTemplateRevisionId::from_uuid)
            .map_err(|_| "Workflow ExecutionTemplate revision identity is invalid".to_owned())?;
        let effective_input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow execution effective input",
        )?))?;
        let value = Self {
            schema: WORKFLOW_EXECUTION_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_EXECUTION_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
            execution_template_id: ExecutionTemplateId::from_uuid(capability.resource_id),
            execution_template_revision_id: revision_id,
            execution_template_digest: capability.digest.clone(),
            capability: capability.capability.clone(),
            effective_input,
            effective_input_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EXECUTION_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.execution_template_id.as_uuid().is_nil()
            || self.execution_template_revision_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_EXECUTION_STEP_ATTEMPT
            || self.capability != "execution.run"
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Workflow execution hook metadata is invalid".into());
        }
        let digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &self.effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow execution effective input",
        )?))?;
        if digest != self.effective_input_digest {
            return Err("Workflow execution effective input digest does not match".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!("workflow-execution:{}:{}", self.step_id, self.step_attempt)
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-execution:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowExecutionChildReferenceMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub execution_template_id: ExecutionTemplateId,
    pub execution_template_revision_id: ExecutionTemplateRevisionId,
    pub execution_template_digest: Sha256Digest,
    pub invocation_template_digest: Sha256Digest,
}

impl WorkflowExecutionChildReferenceMetadata {
    pub fn new(
        hook: &WorkflowExecutionHookMetadata,
        invocation_template_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA.into(),
            organization_id: hook.organization_id,
            project_id: hook.project_id,
            workflow_run_id: hook.workflow_run_id,
            plan_revision_id: hook.plan_revision_id,
            plan_digest: hook.plan_digest.clone(),
            step_id: hook.step_id.clone(),
            step_attempt: hook.step_attempt,
            execution_template_id: hook.execution_template_id,
            execution_template_revision_id: hook.execution_template_revision_id,
            execution_template_digest: hook.execution_template_digest.clone(),
            invocation_template_digest,
        };
        value.validate(hook)?;
        Ok(value)
    }

    pub fn validate(&self, hook: &WorkflowExecutionHookMetadata) -> Result<(), String> {
        hook.validate()?;
        if self.schema != WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA
            || self.organization_id != hook.organization_id
            || self.project_id != hook.project_id
            || self.workflow_run_id != hook.workflow_run_id
            || self.plan_revision_id != hook.plan_revision_id
            || self.plan_digest != hook.plan_digest
            || self.step_id != hook.step_id
            || self.step_attempt != hook.step_attempt
            || self.execution_template_id != hook.execution_template_id
            || self.execution_template_revision_id != hook.execution_template_revision_id
            || self.execution_template_digest != hook.execution_template_digest
        {
            return Err("Workflow execution child reference authority is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowExecutionOutcome {
    Succeeded {
        exit_code: i32,
    },
    Failed {
        exit_code: Option<i32>,
        reason: String,
    },
    Cancelled,
}

impl WorkflowExecutionOutcome {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Succeeded { exit_code: 0 } | Self::Cancelled => Ok(()),
            Self::Succeeded { .. } => {
                Err("successful Workflow execution must have exit code zero".into())
            }
            Self::Failed { reason, .. }
                if reason.is_empty()
                    || reason.len() > 16 * 1024
                    || reason.contains(['\0', '\r', '\n']) =>
            {
                Err("Workflow execution failure reason is invalid".into())
            }
            Self::Failed { .. } => Ok(()),
        }
    }

    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::Succeeded { exit_code: 0 })
    }

    pub fn failure_message(&self) -> Option<String> {
        match self {
            Self::Succeeded { .. } => None,
            Self::Failed { reason, .. } => Some(reason.clone()),
            Self::Cancelled => Some("child Execution was cancelled".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowExecutionStepOutput {
    pub schema: String,
    pub execution_id: ExecutionId,
    pub operation_id: OperationId,
    pub execution_template_id: ExecutionTemplateId,
    pub execution_template_revision_id: ExecutionTemplateRevisionId,
    pub execution_template_digest: Sha256Digest,
    pub invocation_template_digest: Sha256Digest,
    pub outcome: WorkflowExecutionOutcome,
    pub finished_at: DateTime<Utc>,
}

impl WorkflowExecutionStepOutput {
    pub fn validate(&self, metadata: &WorkflowExecutionHookMetadata) -> Result<(), String> {
        self.validate_shape()?;
        if self.execution_template_id != metadata.execution_template_id
            || self.execution_template_revision_id != metadata.execution_template_revision_id
            || self.execution_template_digest != metadata.execution_template_digest
        {
            return Err("Workflow execution step output authority is invalid".into());
        }
        Ok(())
    }

    pub(super) fn validate_shape(&self) -> Result<(), String> {
        self.outcome.validate()?;
        if self.schema != WORKFLOW_EXECUTION_RESULT_SCHEMA
            || self.execution_id.as_uuid().is_nil()
            || self.operation_id.as_uuid() != self.execution_id.as_uuid()
            || self.finished_at != canonical_timestamp(self.finished_at)
        {
            return Err("Workflow execution step output shape is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowExecutionResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u64,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub resolution: WorkflowExecutionResumeResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowExecutionResumeResolution {
    Completed {
        output: WorkflowExecutionStepOutput,
        output_digest: Sha256Digest,
    },
    Rejected {
        reason: String,
    },
}

impl WorkflowExecutionResumePayload {
    pub fn new(
        metadata: &WorkflowExecutionHookMetadata,
        output: WorkflowExecutionStepOutput,
    ) -> Result<Self, String> {
        output.validate(metadata)?;
        let output_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow execution output",
        )?))?;
        let value = Self {
            schema: WORKFLOW_EXECUTION_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            resolution: WorkflowExecutionResumeResolution::Completed {
                output,
                output_digest,
            },
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn rejected(
        metadata: &WorkflowExecutionHookMetadata,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_EXECUTION_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            resolution: WorkflowExecutionResumeResolution::Rejected {
                reason: reason.into(),
            },
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(&self, metadata: &WorkflowExecutionHookMetadata) -> Result<(), String> {
        metadata.validate()?;
        if self.schema != WORKFLOW_EXECUTION_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
        {
            return Err("Workflow execution resume authority is invalid".into());
        }
        match &self.resolution {
            WorkflowExecutionResumeResolution::Completed {
                output,
                output_digest,
            } => {
                output.validate(metadata)?;
                let digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
                    output,
                    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                    "Workflow execution output",
                )?))?;
                if &digest != output_digest {
                    return Err("Workflow execution output digest does not match".into());
                }
            }
            WorkflowExecutionResumeResolution::Rejected { reason }
                if reason.is_empty()
                    || reason.len() > 16 * 1024
                    || reason.contains(['\0', '\r', '\n']) =>
            {
                return Err("Workflow execution rejection reason is invalid".into());
            }
            WorkflowExecutionResumeResolution::Rejected { .. } => {}
        }
        Ok(())
    }
}
