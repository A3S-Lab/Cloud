use super::{
    CapabilityType, ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind,
    WORKFLOW_RUN_INPUT_MAX_BYTES, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, AgentConversationId,
    AgentExecutionId, AssetId, AssetReleaseId, EnvironmentId, OperationId, OrganizationId,
    PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_AGENT_HOOK_SCHEMA: &str = "cloud.workflow.agent-hook.v1";
pub const WORKFLOW_AGENT_CHILD_REFERENCE_SCHEMA: &str = "cloud.workflow.agent-child-reference.v1";
pub const WORKFLOW_AGENT_RESUME_SCHEMA: &str = "cloud.workflow.agent-resume.v1";
pub const WORKFLOW_AGENT_RESULT_SCHEMA: &str = "cloud.workflow.agent-result.v1";
pub const WORKFLOW_AGENT_STEP_ATTEMPT: u64 = 1;
const WORKFLOW_AGENT_FAILURE_MAX_BYTES: usize = 16 * 1024;
const WORKFLOW_AGENT_PROVIDER_TEXT_MAX_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentHookMetadata {
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
    pub agent_asset_id: AssetId,
    pub agent_asset_release_id: AssetReleaseId,
    pub agent_release_digest: Sha256Digest,
    pub capability: String,
    pub effective_input: serde_json::Value,
    pub effective_input_digest: Sha256Digest,
}

impl WorkflowAgentHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        effective_input: serde_json::Value,
    ) -> Result<Self, String> {
        if step.plan.kind != WorkflowStepKind::Agent {
            return Err("Workflow Agent hook requires an Agent step".into());
        }
        let environment_id = input.plan.environment_id.ok_or_else(|| {
            "Workflow Agent step requires one exact target environment".to_owned()
        })?;
        let capability = step
            .plan
            .capability
            .as_ref()
            .ok_or_else(|| "Workflow Agent step lost its AgentRelease".to_owned())?;
        capability.validate()?;
        if capability.capability_type != CapabilityType::AgentRelease {
            return Err("Workflow Agent step has the wrong capability type".into());
        }
        let release_id = uuid::Uuid::parse_str(&capability.revision)
            .map(AssetReleaseId::from_uuid)
            .map_err(|_| "Workflow AgentRelease revision identity is invalid".to_owned())?;
        let effective_input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow Agent effective input",
        )?))?;
        let value = Self {
            schema: WORKFLOW_AGENT_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_AGENT_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
            agent_asset_id: AssetId::from_uuid(capability.resource_id),
            agent_asset_release_id: release_id,
            agent_release_digest: capability.digest.clone(),
            capability: capability.capability.clone(),
            effective_input,
            effective_input_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_AGENT_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.agent_asset_id.as_uuid().is_nil()
            || self.agent_asset_release_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_AGENT_STEP_ATTEMPT
            || self.capability != "agent.execute"
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Workflow Agent hook metadata is invalid".into());
        }
        let digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &self.effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow Agent effective input",
        )?))?;
        if digest != self.effective_input_digest {
            return Err("Workflow Agent effective input digest does not match".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!("workflow-agent:{}:{}", self.step_id, self.step_attempt)
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-agent:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentChildReferenceMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub agent_asset_id: AssetId,
    pub agent_asset_release_id: AssetReleaseId,
    pub agent_release_digest: Sha256Digest,
    pub conversation_id: AgentConversationId,
    pub agent_execution_id: AgentExecutionId,
    pub operation_id: OperationId,
}

impl WorkflowAgentChildReferenceMetadata {
    pub fn new(
        hook: &WorkflowAgentHookMetadata,
        conversation_id: AgentConversationId,
        agent_execution_id: AgentExecutionId,
        operation_id: OperationId,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_AGENT_CHILD_REFERENCE_SCHEMA.into(),
            organization_id: hook.organization_id,
            project_id: hook.project_id,
            environment_id: hook.environment_id,
            workflow_run_id: hook.workflow_run_id,
            plan_revision_id: hook.plan_revision_id,
            plan_digest: hook.plan_digest.clone(),
            step_id: hook.step_id.clone(),
            step_attempt: hook.step_attempt,
            agent_asset_id: hook.agent_asset_id,
            agent_asset_release_id: hook.agent_asset_release_id,
            agent_release_digest: hook.agent_release_digest.clone(),
            conversation_id,
            agent_execution_id,
            operation_id,
        };
        value.validate(hook)?;
        Ok(value)
    }

    pub fn validate(&self, hook: &WorkflowAgentHookMetadata) -> Result<(), String> {
        hook.validate()?;
        if self.schema != WORKFLOW_AGENT_CHILD_REFERENCE_SCHEMA
            || self.organization_id != hook.organization_id
            || self.project_id != hook.project_id
            || self.environment_id != hook.environment_id
            || self.workflow_run_id != hook.workflow_run_id
            || self.plan_revision_id != hook.plan_revision_id
            || self.plan_digest != hook.plan_digest
            || self.step_id != hook.step_id
            || self.step_attempt != hook.step_attempt
            || self.agent_asset_id != hook.agent_asset_id
            || self.agent_asset_release_id != hook.agent_asset_release_id
            || self.agent_release_digest != hook.agent_release_digest
            || self.conversation_id.as_uuid().is_nil()
            || self.agent_execution_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
        {
            return Err("Workflow Agent child reference authority is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentProviderEvidence {
    pub kind: String,
    pub revision: String,
    pub protocol: String,
    pub native_protocol: String,
    pub profile_digest: Sha256Digest,
    pub capability_digest: Sha256Digest,
    pub session_id: String,
    pub run_id: String,
}

impl WorkflowAgentProviderEvidence {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("kind", self.kind.as_str()),
            ("revision", self.revision.as_str()),
            ("protocol", self.protocol.as_str()),
            ("native protocol", self.native_protocol.as_str()),
            ("session ID", self.session_id.as_str()),
            ("run ID", self.run_id.as_str()),
        ] {
            if value.is_empty()
                || value.len() > WORKFLOW_AGENT_PROVIDER_TEXT_MAX_BYTES
                || value.contains(['\0', '\r', '\n'])
            {
                return Err(format!("Workflow Agent provider {name} is invalid"));
            }
        }
        if !self.kind.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        }) || Sha256Digest::parse(self.profile_digest.as_str())? != self.profile_digest
            || Sha256Digest::parse(self.capability_digest.as_str())? != self.capability_digest
        {
            return Err("Workflow Agent provider evidence is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowAgentOutcome {
    Succeeded,
    Failed { reason: String },
    Cancelled,
}

impl WorkflowAgentOutcome {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Failed { reason } => validate_failure(reason),
            Self::Succeeded | Self::Cancelled => Ok(()),
        }
    }

    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    pub fn failure_message(&self) -> Option<String> {
        match self {
            Self::Succeeded => None,
            Self::Failed { reason } => Some(reason.clone()),
            Self::Cancelled => Some("child Agent execution was cancelled".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentStepOutput {
    pub schema: String,
    pub conversation_id: AgentConversationId,
    pub agent_execution_id: AgentExecutionId,
    pub operation_id: OperationId,
    pub agent_asset_id: AssetId,
    pub agent_asset_release_id: AssetReleaseId,
    pub agent_release_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<WorkflowAgentProviderEvidence>,
    pub outcome: WorkflowAgentOutcome,
    pub text: String,
    pub terminal_event_sequence: u64,
    pub finished_at: DateTime<Utc>,
}

impl WorkflowAgentStepOutput {
    pub fn validate(&self, metadata: &WorkflowAgentHookMetadata) -> Result<(), String> {
        self.validate_shape()?;
        if self.agent_asset_id != metadata.agent_asset_id
            || self.agent_asset_release_id != metadata.agent_asset_release_id
            || self.agent_release_digest != metadata.agent_release_digest
        {
            return Err("Workflow Agent step output authority is invalid".into());
        }
        Ok(())
    }

    pub(super) fn validate_shape(&self) -> Result<(), String> {
        self.outcome.validate()?;
        self.provider
            .as_ref()
            .map(WorkflowAgentProviderEvidence::validate)
            .transpose()?;
        if self.schema != WORKFLOW_AGENT_RESULT_SCHEMA
            || self.conversation_id.as_uuid().is_nil()
            || self.agent_execution_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.agent_asset_id.as_uuid().is_nil()
            || self.agent_asset_release_id.as_uuid().is_nil()
            || self.terminal_event_sequence == 0
            || self.finished_at != canonical_timestamp(self.finished_at)
            || (self.outcome.succeeded() && self.provider.is_none())
        {
            return Err("Workflow Agent step output shape is invalid".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Agent step output",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u64,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub resolution: WorkflowAgentResumeResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowAgentResumeResolution {
    Completed {
        output: Box<WorkflowAgentStepOutput>,
        output_digest: Sha256Digest,
    },
    Rejected {
        reason: String,
    },
}

impl WorkflowAgentResumePayload {
    pub fn new(
        metadata: &WorkflowAgentHookMetadata,
        output: WorkflowAgentStepOutput,
    ) -> Result<Self, String> {
        output.validate(metadata)?;
        let output_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
            &output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Agent output",
        )?))?;
        let value = Self {
            schema: WORKFLOW_AGENT_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            resolution: WorkflowAgentResumeResolution::Completed {
                output: Box::new(output),
                output_digest,
            },
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn rejected(
        metadata: &WorkflowAgentHookMetadata,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_AGENT_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            resolution: WorkflowAgentResumeResolution::Rejected {
                reason: reason.into(),
            },
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(&self, metadata: &WorkflowAgentHookMetadata) -> Result<(), String> {
        metadata.validate()?;
        if self.schema != WORKFLOW_AGENT_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
        {
            return Err("Workflow Agent resume authority is invalid".into());
        }
        match &self.resolution {
            WorkflowAgentResumeResolution::Completed {
                output,
                output_digest,
            } => {
                output.validate(metadata)?;
                let digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
                    output,
                    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                    "Workflow Agent output",
                )?))?;
                if &digest != output_digest {
                    return Err("Workflow Agent output digest does not match".into());
                }
            }
            WorkflowAgentResumeResolution::Rejected { reason } => validate_failure(reason)?,
        }
        Ok(())
    }
}

fn validate_failure(reason: &str) -> Result<(), String> {
    if reason.is_empty()
        || reason.len() > WORKFLOW_AGENT_FAILURE_MAX_BYTES
        || reason.contains(['\0', '\r', '\n'])
    {
        return Err("Workflow Agent failure reason is invalid".into());
    }
    Ok(())
}
