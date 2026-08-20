use super::{ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_HUMAN_DECISION_HOOK_SCHEMA: &str = "cloud.workflow.human-decision-hook.v1";
pub const WORKFLOW_HUMAN_DECISION_STEP_ATTEMPT: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowHumanDecisionHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub configuration_digest: Sha256Digest,
    pub form_id: FormId,
    pub form_release_id: FormReleaseId,
    pub form_release_digest: Sha256Digest,
}

impl WorkflowHumanDecisionHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
    ) -> Result<Self, String> {
        if step.plan.kind != WorkflowStepKind::HumanDecision {
            return Err("Workflow hook metadata requires a human-decision step".into());
        }
        let capability = step
            .plan
            .capability
            .as_ref()
            .ok_or_else(|| "Workflow human-decision step lost its FormRelease".to_owned())?;
        capability.validate()?;
        if capability.capability_type != super::CapabilityType::FormRelease {
            return Err("Workflow human-decision step has the wrong capability type".into());
        }
        let form_release_id = uuid::Uuid::parse_str(&capability.revision)
            .map(FormReleaseId::from_uuid)
            .map_err(|error| format!("Workflow FormRelease identity is invalid: {error}"))?;
        let value = Self {
            schema: WORKFLOW_HUMAN_DECISION_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_HUMAN_DECISION_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
            form_id: FormId::from_uuid(capability.resource_id),
            form_release_id,
            form_release_digest: capability.digest.clone(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_HUMAN_DECISION_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.form_id.as_uuid().is_nil()
            || self.form_release_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_HUMAN_DECISION_STEP_ATTEMPT
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Workflow human-decision hook metadata is invalid".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!("workflow-human:{}:{}", self.step_id, self.step_attempt)
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-human:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}
