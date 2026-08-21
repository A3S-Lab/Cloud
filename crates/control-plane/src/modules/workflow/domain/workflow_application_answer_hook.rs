use super::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ApplicationMessageId, OrganizationId, PlanRevisionId, ProjectId,
    Sha256Digest, WorkflowRunId,
};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_APPLICATION_ANSWER_HOOK_SCHEMA: &str =
    "cloud.workflow.application-answer-hook.v1";
pub const WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA: &str =
    "cloud.workflow.application-answer-resume.v1";
pub const WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationAnswerHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u32,
    pub configuration_digest: Sha256Digest,
    pub content: serde_json::Value,
    pub content_digest: Sha256Digest,
}

impl WorkflowApplicationAnswerHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        content: serde_json::Value,
    ) -> Result<Self, String> {
        let projection = input.application_projection.as_ref().ok_or_else(|| {
            "Workflow Application Answer requires an immutable Application projection".to_owned()
        })?;
        if !projection.is_answer_step(&step.plan.id) || step.plan.kind != WorkflowStepKind::Output {
            return Err(
                "Workflow Application Answer hook requires an exact projected Answer step".into(),
            );
        }
        let content_digest = value_digest(&content)?;
        let value = Self {
            schema: WORKFLOW_APPLICATION_ANSWER_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
            content,
            content_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_APPLICATION_ANSWER_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || Sha256Digest::parse(self.plan_digest.as_str())? != self.plan_digest
            || Sha256Digest::parse(self.configuration_digest.as_str())? != self.configuration_digest
            || value_digest(&self.content)? != self.content_digest
        {
            return Err("Workflow Application Answer hook metadata is invalid".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!(
            "workflow-application-answer:{}:{}",
            self.step_id, self.step_attempt
        )
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-application-answer:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationAnswerResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub message_id: ApplicationMessageId,
    pub message_sequence: u64,
    pub content_digest: Sha256Digest,
}

impl WorkflowApplicationAnswerResumePayload {
    pub fn new(
        metadata: &WorkflowApplicationAnswerHookMetadata,
        message_id: ApplicationMessageId,
        message_sequence: u64,
        content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            message_id,
            message_sequence,
            content_digest,
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(&self, metadata: &WorkflowApplicationAnswerHookMetadata) -> Result<(), String> {
        metadata.validate()?;
        if self.schema != WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || self.message_id.as_uuid().is_nil()
            || self.message_sequence == 0
            || self.content_digest != metadata.content_digest
        {
            return Err("Workflow Application Answer resume authority is invalid".into());
        }
        Ok(())
    }
}

fn value_digest(value: &serde_json::Value) -> Result<Sha256Digest, String> {
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        value,
        WORKFLOW_RUN_OUTPUT_MAX_BYTES,
        "Workflow Application Answer content",
    )?))
}
