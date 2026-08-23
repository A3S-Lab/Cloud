use super::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepFailureClassification, WorkflowStepKind,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5,
    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, ConversationVariableRevisionId, OrganizationId, PlanRevisionId,
    ProjectId, Sha256Digest, WorkflowRunId,
};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_HOOK_SCHEMA: &str =
    "cloud.workflow.application-variable-snapshot-hook.v1";
pub const WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_RESUME_SCHEMA: &str =
    "cloud.workflow.application-variable-snapshot-resume.v1";
pub const WORKFLOW_APPLICATION_VARIABLE_WRITE_HOOK_SCHEMA: &str =
    "cloud.workflow.application-variable-write-hook.v1";
pub const WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA: &str =
    "cloud.workflow.application-variable-write-resume.v1";
pub const WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA: &str =
    "cloud.workflow.application-variable-write-failure-resume.v1";
pub const WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableSnapshotHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub variable_contract_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u32,
    pub configuration_digest: Sha256Digest,
}

impl WorkflowApplicationVariableSnapshotHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
    ) -> Result<Self, String> {
        let projection = input.application_projection.as_ref().ok_or_else(|| {
            "Workflow Application variable read requires an immutable Application projection"
                .to_owned()
        })?;
        let variable_contract_digest = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| {
                "Workflow Application variable read lost its immutable variable contract".to_owned()
            })?
            .digest
            .clone();
        if !matches!(
            projection.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) || !projection.is_variable_step(&step.plan.id)
            || !matches!(
                step.plan.kind,
                WorkflowStepKind::Output | WorkflowStepKind::Service
            )
        {
            return Err(
                "Workflow Application variable snapshot requires an exact projected variable port"
                    .into(),
            );
        }
        let value = Self {
            schema: WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            variable_contract_digest,
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT
            || !valid_step_id(&self.step_id)
            || Sha256Digest::parse(self.plan_digest.as_str())? != self.plan_digest
            || Sha256Digest::parse(self.variable_contract_digest.as_str())?
                != self.variable_contract_digest
            || Sha256Digest::parse(self.configuration_digest.as_str())? != self.configuration_digest
        {
            return Err("Workflow Application variable snapshot hook metadata is invalid".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!(
            "workflow-application-variable-snapshot:{}:{}",
            self.step_id, self.step_attempt
        )
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-application-variable-snapshot:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableSnapshotResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub revision_id: ConversationVariableRevisionId,
    pub revision_number: u64,
    pub values_digest: Sha256Digest,
    pub values: serde_json::Value,
}

impl WorkflowApplicationVariableSnapshotResumePayload {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metadata: &WorkflowApplicationVariableSnapshotHookMetadata,
        application_id: ApplicationId,
        application_release_id: ApplicationReleaseId,
        application_release_digest: Sha256Digest,
        session_id: ApplicationSessionId,
        invocation_id: ApplicationInvocationId,
        revision_id: ConversationVariableRevisionId,
        revision_number: u64,
        values_digest: Sha256Digest,
        values: serde_json::Value,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            application_id,
            application_release_id,
            application_release_digest,
            session_id,
            invocation_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            revision_id,
            revision_number,
            values_digest,
            values,
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowApplicationVariableSnapshotHookMetadata,
    ) -> Result<(), String> {
        metadata.validate()?;
        if self.schema != WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || self.revision_id.as_uuid().is_nil()
            || self.revision_number == 0
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || value_digest(&self.values)? != self.values_digest
        {
            return Err(
                "Workflow Application variable snapshot resume authority is invalid".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableWriteHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub variable_contract_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u32,
    pub configuration_digest: Sha256Digest,
    pub expected_revision_id: ConversationVariableRevisionId,
    pub expected_revision_number: u64,
    pub expected_values_digest: Sha256Digest,
    pub values_digest: Sha256Digest,
}

impl WorkflowApplicationVariableWriteHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        snapshot: &WorkflowApplicationVariableSnapshotResumePayload,
        values: &serde_json::Value,
    ) -> Result<Self, String> {
        let projection = input.application_projection.as_ref().ok_or_else(|| {
            "Workflow Application variable write requires an immutable Application projection"
                .to_owned()
        })?;
        let snapshot_metadata =
            WorkflowApplicationVariableSnapshotHookMetadata::from_run_step(input, step)?;
        snapshot.validate(&snapshot_metadata)?;
        if !matches!(
            projection.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) || !projection.is_variable_assignment_step(&step.plan.id)
            || step.plan.kind != WorkflowStepKind::Service
        {
            return Err(
                "Workflow Application variable write requires an exact projected assignment port"
                    .into(),
            );
        }
        let values_digest = value_digest(values)?;
        if values_digest == snapshot.values_digest {
            return Err(
                "Workflow Application variable assignment must change the canonical values".into(),
            );
        }
        let value = Self {
            schema: WORKFLOW_APPLICATION_VARIABLE_WRITE_HOOK_SCHEMA.into(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            application_id: snapshot.application_id,
            application_release_id: snapshot.application_release_id,
            application_release_digest: snapshot.application_release_digest.clone(),
            session_id: snapshot.session_id,
            invocation_id: snapshot.invocation_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            variable_contract_digest: snapshot_metadata.variable_contract_digest,
            step_id: step.plan.id.clone(),
            step_attempt: WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT,
            configuration_digest: step.plan.configuration_digest.clone(),
            expected_revision_id: snapshot.revision_id,
            expected_revision_number: snapshot.revision_number,
            expected_values_digest: snapshot.values_digest.clone(),
            values_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_APPLICATION_VARIABLE_WRITE_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.step_attempt != WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT
            || !valid_step_id(&self.step_id)
            || self.expected_revision_id.as_uuid().is_nil()
            || self.expected_revision_number == 0
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || Sha256Digest::parse(self.plan_digest.as_str())? != self.plan_digest
            || Sha256Digest::parse(self.variable_contract_digest.as_str())?
                != self.variable_contract_digest
            || Sha256Digest::parse(self.configuration_digest.as_str())? != self.configuration_digest
            || Sha256Digest::parse(self.expected_values_digest.as_str())?
                != self.expected_values_digest
            || Sha256Digest::parse(self.values_digest.as_str())? != self.values_digest
            || self.values_digest == self.expected_values_digest
        {
            return Err("Workflow Application variable write hook metadata is invalid".into());
        }
        Ok(())
    }

    pub fn validate_run_step(
        &self,
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        snapshot: &WorkflowApplicationVariableSnapshotResumePayload,
    ) -> Result<(), String> {
        self.validate()?;
        let snapshot_metadata =
            WorkflowApplicationVariableSnapshotHookMetadata::from_run_step(input, step)?;
        snapshot.validate(&snapshot_metadata)?;
        let projection = input.application_projection.as_ref().ok_or_else(|| {
            "Workflow Application variable write lost its immutable projection".to_owned()
        })?;
        if !matches!(
            projection.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) || !projection.is_variable_assignment_step(&step.plan.id)
            || step.plan.kind != WorkflowStepKind::Service
            || self.organization_id != input.organization_id
            || self.project_id != input.project_id
            || self.application_id != snapshot.application_id
            || self.application_release_id != snapshot.application_release_id
            || self.application_release_digest != snapshot.application_release_digest
            || self.session_id != snapshot.session_id
            || self.invocation_id != snapshot.invocation_id
            || self.workflow_run_id != input.workflow_run_id
            || self.plan_revision_id != input.plan_revision_id
            || self.plan_digest != input.plan_digest
            || self.variable_contract_digest != snapshot_metadata.variable_contract_digest
            || self.step_id != step.plan.id
            || self.step_attempt != snapshot.step_attempt
            || self.configuration_digest != step.plan.configuration_digest
            || self.expected_revision_id != snapshot.revision_id
            || self.expected_revision_number != snapshot.revision_number
            || self.expected_values_digest != snapshot.values_digest
        {
            return Err("Workflow Application variable write hook authority drifted".into());
        }
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!(
            "workflow-application-variable-write:{}:{}",
            self.step_id, self.step_attempt
        )
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-application-variable-write:{}:{}:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableWriteResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub revision_id: ConversationVariableRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: ConversationVariableRevisionId,
    pub parent_digest: Sha256Digest,
    pub values_digest: Sha256Digest,
}

impl WorkflowApplicationVariableWriteResumePayload {
    pub fn new(
        metadata: &WorkflowApplicationVariableWriteHookMetadata,
        revision_id: ConversationVariableRevisionId,
        revision_number: u64,
        parent_revision_id: ConversationVariableRevisionId,
        parent_digest: Sha256Digest,
        values_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            revision_id,
            revision_number,
            parent_revision_id,
            parent_digest,
            values_digest,
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowApplicationVariableWriteHookMetadata,
    ) -> Result<(), String> {
        metadata.validate()?;
        let expected_revision_number = metadata
            .expected_revision_number
            .checked_add(1)
            .ok_or_else(|| {
                "Workflow Application variable revision number is exhausted".to_owned()
            })?;
        if self.schema != WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || self.revision_id.as_uuid().is_nil()
            || self.revision_number != expected_revision_number
            || self.parent_revision_id != metadata.expected_revision_id
            || self.parent_digest != metadata.expected_values_digest
            || self.values_digest != metadata.values_digest
        {
            return Err("Workflow Application variable write resume authority is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableWriteFailureResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub classification: WorkflowStepFailureClassification,
}

impl WorkflowApplicationVariableWriteFailureResumePayload {
    pub fn new(
        metadata: &WorkflowApplicationVariableWriteHookMetadata,
        classification: WorkflowStepFailureClassification,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA.into(),
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            classification,
        };
        value.validate(metadata)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowApplicationVariableWriteHookMetadata,
    ) -> Result<(), String> {
        metadata.validate()?;
        if self.schema != WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || !self.classification.is_application()
        {
            return Err(
                "Workflow Application variable write failure resume authority is invalid".into(),
            );
        }
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Application variable write failure resume",
        )?;
        Ok(())
    }
}

fn valid_step_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn value_digest(value: &serde_json::Value) -> Result<Sha256Digest, String> {
    if !value.is_object() {
        return Err("Workflow Application variables must be a JSON object".into());
    }
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        value,
        WORKFLOW_RUN_OUTPUT_MAX_BYTES,
        "Workflow Application variable values",
    )?))
}
