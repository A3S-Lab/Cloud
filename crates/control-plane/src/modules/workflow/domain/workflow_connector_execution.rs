use super::{
    CapabilityType, ResolvedWorkflowRunStep, WorkflowPolicyMode, WorkflowRetryPolicy,
    WorkflowRunInput, WorkflowStepKind, WORKFLOW_RETRY_MAXIMUM_DEFAULT_DELAY_SECONDS,
    WORKFLOW_RUN_INPUT_MAX_BYTES, WORKFLOW_RUN_INPUT_SCHEMA_V10, WORKFLOW_RUN_INPUT_SCHEMA_V13,
    WORKFLOW_RUN_INPUT_SCHEMA_V23, WORKFLOW_RUN_INPUT_SCHEMA_V6, WORKFLOW_RUN_INPUT_SCHEMA_V8,
    WORKFLOW_RUN_INPUT_SCHEMA_V9, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, ConnectorProfileId,
    ConnectorRevisionId, EnvironmentId, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_CONNECTOR_HOOK_SCHEMA: &str = "cloud.workflow.connector-hook.v1";
pub const WORKFLOW_CONNECTOR_HOOK_SCHEMA_V2: &str = "cloud.workflow.connector-hook.v2";
pub const WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3: &str = "cloud.workflow.connector-hook.v3";
pub const WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4: &str = "cloud.workflow.connector-hook.v4";
pub const WORKFLOW_CONNECTOR_RESUME_SCHEMA: &str = "cloud.workflow.connector-resume.v1";
pub const WORKFLOW_CONNECTOR_RESUME_SCHEMA_V2: &str = "cloud.workflow.connector-resume.v2";
pub const WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA: &str = "cloud.workflow.connector-evidence.v1";
pub const WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2: &str = "cloud.workflow.connector-evidence.v2";
pub const WORKFLOW_CONNECTOR_RESULT_SCHEMA: &str = "cloud.workflow.connector-result.v1";
pub const WORKFLOW_CONNECTOR_RESULT_SCHEMA_V2: &str = "cloud.workflow.connector-result.v2";
pub const WORKFLOW_CONNECTOR_RESPONSE_OBJECT_SCHEMA: &str =
    "cloud.workflow.connector-response-object.v1";
pub const WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT: u32 = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowConnectorInvocationPurpose {
    #[default]
    Normal,
    CancellationCompensation {
        source_step_id: String,
    },
}

impl WorkflowConnectorInvocationPurpose {
    fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnectorHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u32,
    pub observation: u32,
    pub configuration_digest: Sha256Digest,
    pub connector_profile_id: ConnectorProfileId,
    pub connector_revision_id: ConnectorRevisionId,
    pub connector_revision_digest: Sha256Digest,
    pub capability: String,
    pub effective_input: serde_json::Value,
    pub effective_input_digest: Sha256Digest,
    pub retry_policy_digest: Sha256Digest,
    pub retry_policy: WorkflowRetryPolicy,
    #[serde(
        default,
        skip_serializing_if = "WorkflowConnectorInvocationPurpose::is_normal"
    )]
    pub purpose: WorkflowConnectorInvocationPurpose,
}

impl WorkflowConnectorHookMetadata {
    pub fn from_run_step(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        effective_input: serde_json::Value,
        step_attempt: u32,
        observation: u32,
    ) -> Result<Self, String> {
        Self::build(
            input,
            step,
            effective_input,
            step_attempt,
            observation,
            WorkflowConnectorInvocationPurpose::Normal,
        )
    }

    pub fn from_cancellation_compensation(
        input: &WorkflowRunInput,
        source: &ResolvedWorkflowRunStep,
        target: &ResolvedWorkflowRunStep,
        effective_input: serde_json::Value,
        step_attempt: u32,
        observation: u32,
    ) -> Result<Self, String> {
        if input.schema != WORKFLOW_RUN_INPUT_SCHEMA_V23
            || source
                .policy
                .as_ref()
                .and_then(|policy| policy.cancellation_compensation.as_ref())
                .is_none_or(|compensation| compensation.step_id != target.plan.id)
        {
            return Err(
                "Workflow Connector cancellation compensation lost its immutable policy authority"
                    .into(),
            );
        }
        Self::build(
            input,
            target,
            effective_input,
            step_attempt,
            observation,
            WorkflowConnectorInvocationPurpose::CancellationCompensation {
                source_step_id: source.plan.id.clone(),
            },
        )
    }

    fn build(
        input: &WorkflowRunInput,
        step: &ResolvedWorkflowRunStep,
        effective_input: serde_json::Value,
        step_attempt: u32,
        observation: u32,
        purpose: WorkflowConnectorInvocationPurpose,
    ) -> Result<Self, String> {
        if step.plan.kind != WorkflowStepKind::Service {
            return Err("Workflow Connector hook requires a Service step".into());
        }
        let environment_id = input.plan.environment_id.ok_or_else(|| {
            "Workflow Connector step requires one exact target environment".to_owned()
        })?;
        let capability = step
            .plan
            .capability
            .as_ref()
            .ok_or_else(|| "Workflow Connector step lost its exact revision".to_owned())?;
        capability.validate()?;
        if capability.capability_type != CapabilityType::ConnectorRevision {
            return Err("Workflow Connector step has the wrong capability type".into());
        }
        let connector_revision_id = Uuid::parse_str(&capability.revision)
            .map(ConnectorRevisionId::from_uuid)
            .map_err(|_| "Workflow Connector revision identity is invalid".to_owned())?;
        let retry_policy = step
            .policy
            .as_ref()
            .filter(|policy| policy.mode == WorkflowPolicyMode::Static)
            .and_then(|policy| policy.retry)
            .ok_or_else(|| "Workflow Connector step lost its retry policy".to_owned())?;
        let retry_policy_digest = step
            .plan
            .policy_digest
            .clone()
            .ok_or_else(|| "Workflow Connector step lost its retry policy digest".to_owned())?;
        let canonical_input = canonical_json_bounded(
            &effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow Connector effective input",
        )?;
        let value = Self {
            schema: if !purpose.is_normal() {
                WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4.into()
            } else {
                match input.schema.as_str() {
                    WORKFLOW_RUN_INPUT_SCHEMA_V8
                    | WORKFLOW_RUN_INPUT_SCHEMA_V9
                    | WORKFLOW_RUN_INPUT_SCHEMA_V10
                    | WORKFLOW_RUN_INPUT_SCHEMA_V13
                    | WORKFLOW_RUN_INPUT_SCHEMA_V23 => WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3.into(),
                    WORKFLOW_RUN_INPUT_SCHEMA_V6 => WORKFLOW_CONNECTOR_HOOK_SCHEMA_V2.into(),
                    _ => WORKFLOW_CONNECTOR_HOOK_SCHEMA.into(),
                }
            },
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            step_id: step.plan.id.clone(),
            step_attempt,
            observation,
            configuration_digest: step.plan.configuration_digest.clone(),
            connector_profile_id: ConnectorProfileId::from_uuid(capability.resource_id),
            connector_revision_id,
            connector_revision_digest: capability.digest.clone(),
            capability: capability.capability.clone(),
            effective_input,
            effective_input_digest: Sha256Digest::parse(sha256_digest(&canonical_input))?,
            retry_policy_digest,
            retry_policy,
            purpose,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.schema.as_str(),
            WORKFLOW_CONNECTOR_HOOK_SCHEMA
                | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V2
                | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3
                | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4
        ) || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.connector_profile_id.as_uuid().is_nil()
            || self.connector_revision_id.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.observation == 0
            || self.observation > WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT
            || self.capability != "connector.http"
            || !valid_step_id(&self.step_id)
        {
            return Err("Workflow Connector hook metadata is invalid".into());
        }
        self.retry_policy.validate()?;
        match (&self.purpose, self.schema.as_str()) {
            (WorkflowConnectorInvocationPurpose::Normal, schema)
                if schema != WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4 => {}
            (
                WorkflowConnectorInvocationPurpose::CancellationCompensation { source_step_id },
                WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4,
            ) if valid_step_id(source_step_id) && source_step_id != &self.step_id => {}
            _ => return Err("Workflow Connector hook purpose is invalid".into()),
        }
        Sha256Digest::parse(self.plan_digest.as_str())?;
        Sha256Digest::parse(self.configuration_digest.as_str())?;
        Sha256Digest::parse(self.connector_revision_digest.as_str())?;
        Sha256Digest::parse(self.retry_policy_digest.as_str())?;
        let canonical_input = canonical_json_bounded(
            &self.effective_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow Connector effective input",
        )?;
        if Sha256Digest::parse(sha256_digest(&canonical_input))? != self.effective_input_digest {
            return Err("Workflow Connector effective input digest does not match".into());
        }
        Ok(())
    }

    pub fn requires_response_object(&self) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_CONNECTOR_HOOK_SCHEMA_V2
                | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3
                | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4
        )
    }

    pub fn requires_typed_response(&self) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3 | WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4
        )
    }

    pub fn flow_hook_id(&self) -> String {
        match &self.purpose {
            WorkflowConnectorInvocationPurpose::Normal => format!(
                "workflow-connector:{}:{}:{}",
                self.step_id, self.step_attempt, self.observation
            ),
            WorkflowConnectorInvocationPurpose::CancellationCompensation { source_step_id } => {
                format!(
                    "workflow-connector-cancellation-compensation:{source_step_id}:{}:{}:{}",
                    self.step_id, self.step_attempt, self.observation
                )
            }
        }
    }

    pub fn flow_hook_token(&self) -> String {
        match &self.purpose {
            WorkflowConnectorInvocationPurpose::Normal => format!(
                "workflow-connector:{}:{}:{}:{}",
                self.workflow_run_id, self.step_id, self.step_attempt, self.observation
            ),
            WorkflowConnectorInvocationPurpose::CancellationCompensation { source_step_id } => {
                format!(
                    "workflow-connector-cancellation-compensation:{}:{source_step_id}:{}:{}:{}",
                    self.workflow_run_id, self.step_id, self.step_attempt, self.observation
                )
            }
        }
    }

    pub fn observation_wait_id(&self) -> String {
        match &self.purpose {
            WorkflowConnectorInvocationPurpose::Normal => format!(
                "workflow-connector-observe:{}:{}:{}",
                self.step_id, self.step_attempt, self.observation
            ),
            WorkflowConnectorInvocationPurpose::CancellationCompensation { source_step_id } => {
                format!(
                "workflow-connector-cancellation-compensation-observe:{source_step_id}:{}:{}:{}",
                self.step_id, self.step_attempt, self.observation
            )
            }
        }
    }

    pub fn retry_wait_id(&self) -> String {
        match &self.purpose {
            WorkflowConnectorInvocationPurpose::Normal => format!(
                "workflow-connector-retry:{}:{}",
                self.step_id, self.step_attempt
            ),
            WorkflowConnectorInvocationPurpose::CancellationCompensation { source_step_id } => {
                format!(
                    "workflow-connector-cancellation-compensation-retry:{source_step_id}:{}:{}",
                    self.step_id, self.step_attempt
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowConnectorAttemptOutcome {
    Accepted,
    Retryable,
    Rejected,
}

impl WorkflowConnectorAttemptOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Retryable => "retryable",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnectorResponseObjectReference {
    pub schema: String,
    pub connector_attempt_id: Uuid,
    pub object_ref: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
}

impl WorkflowConnectorResponseObjectReference {
    pub fn new(
        connector_attempt_id: Uuid,
        object_ref: impl Into<String>,
        digest: Sha256Digest,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_CONNECTOR_RESPONSE_OBJECT_SCHEMA.into(),
            connector_attempt_id,
            object_ref: object_ref.into(),
            digest,
            size_bytes,
        };
        value.validate(connector_attempt_id)?;
        Ok(value)
    }

    pub fn validate(&self, expected_attempt_id: Uuid) -> Result<(), String> {
        if self.schema != WORKFLOW_CONNECTOR_RESPONSE_OBJECT_SCHEMA
            || self.connector_attempt_id.is_nil()
            || self.connector_attempt_id != expected_attempt_id
            || self.size_bytes > WORKFLOW_RUN_INPUT_MAX_BYTES as u64
        {
            return Err("Workflow Connector response-object reference is invalid".into());
        }
        let digest = Sha256Digest::parse(self.digest.as_str())?;
        let hexadecimal = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| "Workflow Connector response-object digest is invalid".to_owned())?;
        let expected_ref = format!("attempts/{expected_attempt_id}/sha256/{hexadecimal}/body");
        if self.object_ref != expected_ref {
            return Err("Workflow Connector response-object path changed its authority".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnectorAttemptEvidence {
    pub schema: String,
    pub connector_attempt_id: Uuid,
    pub request_digest: Sha256Digest,
    pub request_body_bytes: u64,
    pub outcome: WorkflowConnectorAttemptOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_digest: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_object: Option<WorkflowConnectorResponseObjectReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

impl WorkflowConnectorAttemptEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        connector_attempt_id: Uuid,
        request_digest: Sha256Digest,
        request_body_bytes: u64,
        outcome: WorkflowConnectorAttemptOutcome,
        response_status: Option<u16>,
        response_digest: Option<Sha256Digest>,
        response_body_bytes: Option<u64>,
        retry_after_seconds: Option<u64>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::restore_version(
            WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA,
            connector_attempt_id,
            request_digest,
            request_body_bytes,
            outcome,
            response_status,
            response_digest,
            response_body_bytes,
            None,
            retry_after_seconds,
            started_at,
            completed_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_with_response_object(
        connector_attempt_id: Uuid,
        request_digest: Sha256Digest,
        request_body_bytes: u64,
        outcome: WorkflowConnectorAttemptOutcome,
        response_status: Option<u16>,
        response_digest: Option<Sha256Digest>,
        response_body_bytes: Option<u64>,
        response_object: Option<WorkflowConnectorResponseObjectReference>,
        retry_after_seconds: Option<u64>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::restore_version(
            WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2,
            connector_attempt_id,
            request_digest,
            request_body_bytes,
            outcome,
            response_status,
            response_digest,
            response_body_bytes,
            response_object,
            retry_after_seconds,
            started_at,
            completed_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn restore_version(
        schema: &str,
        connector_attempt_id: Uuid,
        request_digest: Sha256Digest,
        request_body_bytes: u64,
        outcome: WorkflowConnectorAttemptOutcome,
        response_status: Option<u16>,
        response_digest: Option<Sha256Digest>,
        response_body_bytes: Option<u64>,
        response_object: Option<WorkflowConnectorResponseObjectReference>,
        retry_after_seconds: Option<u64>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            schema: schema.into(),
            connector_attempt_id,
            request_digest,
            request_body_bytes,
            outcome,
            response_status,
            response_digest,
            response_body_bytes,
            response_object,
            retry_after_seconds,
            started_at: canonical_timestamp(started_at),
            completed_at: canonical_timestamp(completed_at),
        };
        value.validate_shape()?;
        Ok(value)
    }

    pub fn validate_authority(
        &self,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<(), String> {
        self.validate_shape()?;
        if self.connector_attempt_id != expected_attempt_id
            || &self.request_digest != expected_request_digest
            || self.request_body_bytes != expected_request_body_bytes
        {
            return Err("Workflow Connector evidence changed its request authority".into());
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), String> {
        if !matches!(
            self.schema.as_str(),
            WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA | WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2
        ) || self.connector_attempt_id.is_nil()
            || self.started_at != canonical_timestamp(self.started_at)
            || self.completed_at != canonical_timestamp(self.completed_at)
            || self.completed_at < self.started_at
        {
            return Err("Workflow Connector evidence identity or time is invalid".into());
        }
        Sha256Digest::parse(self.request_digest.as_str())?;
        if self
            .response_status
            .is_some_and(|status| !(100..=599).contains(&status))
            || self
                .response_body_bytes
                .is_some_and(|bytes| bytes > WORKFLOW_RUN_INPUT_MAX_BYTES as u64)
            || self
                .retry_after_seconds
                .is_some_and(|seconds| seconds > WORKFLOW_RETRY_MAXIMUM_DEFAULT_DELAY_SECONDS)
            || self.response_digest.as_ref().is_some_and(|digest| {
                Sha256Digest::parse(digest.as_str()).ok().as_ref() != Some(digest)
            })
        {
            return Err("Workflow Connector response evidence is invalid".into());
        }
        let response_object_valid = match (
            self.schema.as_str(),
            self.outcome,
            self.response_object.as_ref(),
        ) {
            (WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA, _, None) => true,
            (
                WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2,
                WorkflowConnectorAttemptOutcome::Accepted,
                Some(reference),
            ) => {
                reference.validate(self.connector_attempt_id).is_ok()
                    && Some(&reference.digest) == self.response_digest.as_ref()
                    && Some(reference.size_bytes) == self.response_body_bytes
            }
            (
                WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2,
                WorkflowConnectorAttemptOutcome::Retryable
                | WorkflowConnectorAttemptOutcome::Rejected,
                None,
            ) => true,
            _ => false,
        };
        if !response_object_valid {
            return Err("Workflow Connector response-object evidence is inconsistent".into());
        }
        match self.outcome {
            WorkflowConnectorAttemptOutcome::Accepted
                if self
                    .response_status
                    .is_some_and(|status| (200..=299).contains(&status))
                    && self.response_digest.is_some()
                    && self.response_body_bytes.is_some()
                    && self.retry_after_seconds.is_none() =>
            {
                Ok(())
            }
            WorkflowConnectorAttemptOutcome::Retryable
                if self
                    .response_status
                    .is_none_or(|status| !(200..=299).contains(&status))
                    && self.response_digest.is_none()
                    && self.response_body_bytes.is_none() =>
            {
                Ok(())
            }
            WorkflowConnectorAttemptOutcome::Rejected
                if self
                    .response_status
                    .is_none_or(|status| !(200..=299).contains(&status))
                    && self.response_digest.is_none()
                    && self.response_body_bytes.is_none()
                    && self.retry_after_seconds.is_none() =>
            {
                Ok(())
            }
            _ => Err("Workflow Connector evidence outcome fields are inconsistent".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnectorStepOutput {
    pub schema: String,
    pub connector_profile_id: ConnectorProfileId,
    pub connector_revision_id: ConnectorRevisionId,
    pub connector_revision_digest: Sha256Digest,
    pub connector_attempt_id: Uuid,
    pub response_status: u16,
    pub response_digest: Sha256Digest,
    pub response_body_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_object: Option<WorkflowConnectorResponseObjectReference>,
    pub completed_at: DateTime<Utc>,
}

impl WorkflowConnectorStepOutput {
    pub fn from_evidence(
        metadata: &WorkflowConnectorHookMetadata,
        evidence: &WorkflowConnectorAttemptEvidence,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        metadata.validate()?;
        evidence.validate_authority(
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )?;
        if metadata.requires_response_object()
            != (evidence.schema == WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2)
        {
            return Err(
                "Workflow Connector result changed its response-object contract version".into(),
            );
        }
        if evidence.outcome != WorkflowConnectorAttemptOutcome::Accepted {
            return Err("Workflow Connector result requires accepted evidence".into());
        }
        let value = Self {
            schema: if evidence.response_object.is_some() {
                WORKFLOW_CONNECTOR_RESULT_SCHEMA_V2.into()
            } else {
                WORKFLOW_CONNECTOR_RESULT_SCHEMA.into()
            },
            connector_profile_id: metadata.connector_profile_id,
            connector_revision_id: metadata.connector_revision_id,
            connector_revision_digest: metadata.connector_revision_digest.clone(),
            connector_attempt_id: evidence.connector_attempt_id,
            response_status: evidence
                .response_status
                .ok_or_else(|| "accepted Workflow Connector evidence lost its status".to_owned())?,
            response_digest: evidence.response_digest.clone().ok_or_else(|| {
                "accepted Workflow Connector evidence lost its response digest".to_owned()
            })?,
            response_body_bytes: evidence.response_body_bytes.ok_or_else(|| {
                "accepted Workflow Connector evidence lost its response size".to_owned()
            })?,
            response_object: evidence.response_object.clone(),
            completed_at: evidence.completed_at,
        };
        value.validate(metadata, expected_attempt_id)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowConnectorHookMetadata,
        expected_attempt_id: Uuid,
    ) -> Result<(), String> {
        metadata.validate()?;
        if !matches!(
            self.schema.as_str(),
            WORKFLOW_CONNECTOR_RESULT_SCHEMA | WORKFLOW_CONNECTOR_RESULT_SCHEMA_V2
        ) || self.connector_profile_id != metadata.connector_profile_id
            || self.connector_revision_id != metadata.connector_revision_id
            || self.connector_revision_digest != metadata.connector_revision_digest
            || self.connector_attempt_id != expected_attempt_id
            || !(200..=299).contains(&self.response_status)
            || self.completed_at != canonical_timestamp(self.completed_at)
        {
            return Err("Workflow Connector result authority is invalid".into());
        }
        Sha256Digest::parse(self.response_digest.as_str())?;
        match (self.schema.as_str(), self.response_object.as_ref()) {
            (WORKFLOW_CONNECTOR_RESULT_SCHEMA, None) => {}
            (WORKFLOW_CONNECTOR_RESULT_SCHEMA_V2, Some(reference)) => {
                reference.validate(expected_attempt_id)?;
                if reference.digest != self.response_digest
                    || reference.size_bytes != self.response_body_bytes
                {
                    return Err(
                        "Workflow Connector result response object changed its evidence".into(),
                    );
                }
            }
            _ => {
                return Err(
                    "Workflow Connector result schema and response object are inconsistent".into(),
                )
            }
        }
        canonical_json_bounded(
            self,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Connector result",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnectorResumePayload {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub observation: u32,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub resolution: WorkflowConnectorResumeResolution,
    pub digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowConnectorResumeResolution {
    Completed {
        evidence: Box<WorkflowConnectorAttemptEvidence>,
    },
    Deferred {
        connector_attempt_id: Uuid,
        retry_not_before: DateTime<Utc>,
    },
    Indeterminate {
        connector_attempt_id: Uuid,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowConnectorResumeDigestContent<'a> {
    schema: &'a str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    step_id: &'a str,
    step_attempt: u32,
    observation: u32,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    resolution: &'a WorkflowConnectorResumeResolution,
}

impl WorkflowConnectorResumePayload {
    pub fn completed(
        metadata: &WorkflowConnectorHookMetadata,
        evidence: WorkflowConnectorAttemptEvidence,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        evidence.validate_authority(
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )?;
        Self::build(
            metadata,
            WorkflowConnectorResumeResolution::Completed {
                evidence: Box::new(evidence),
            },
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )
    }

    pub fn deferred(
        metadata: &WorkflowConnectorHookMetadata,
        expected_attempt_id: Uuid,
        retry_not_before: DateTime<Utc>,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        Self::build(
            metadata,
            WorkflowConnectorResumeResolution::Deferred {
                connector_attempt_id: expected_attempt_id,
                retry_not_before: canonical_timestamp(retry_not_before),
            },
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn indeterminate(
        metadata: &WorkflowConnectorHookMetadata,
        expected_attempt_id: Uuid,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        Self::build(
            metadata,
            WorkflowConnectorResumeResolution::Indeterminate {
                connector_attempt_id: expected_attempt_id,
                dispatch_started_at: canonical_timestamp(dispatch_started_at),
                outcome_deadline_at: canonical_timestamp(outcome_deadline_at),
            },
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )
    }

    pub fn rejected(
        metadata: &WorkflowConnectorHookMetadata,
        reason: impl Into<String>,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        Self::build(
            metadata,
            WorkflowConnectorResumeResolution::Rejected {
                reason: reason.into(),
            },
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )
    }

    fn build(
        metadata: &WorkflowConnectorHookMetadata,
        resolution: WorkflowConnectorResumeResolution,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: if metadata.requires_response_object() {
                WORKFLOW_CONNECTOR_RESUME_SCHEMA_V2.into()
            } else {
                WORKFLOW_CONNECTOR_RESUME_SCHEMA.into()
            },
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            step_id: metadata.step_id.clone(),
            step_attempt: metadata.step_attempt,
            observation: metadata.observation,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            resolution,
            digest: zero_digest()?,
        };
        value.digest = value.compute_digest()?;
        value.validate(
            metadata,
            expected_attempt_id,
            expected_request_digest,
            expected_request_body_bytes,
        )?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowConnectorHookMetadata,
        expected_attempt_id: Uuid,
        expected_request_digest: &Sha256Digest,
        expected_request_body_bytes: u64,
    ) -> Result<(), String> {
        metadata.validate()?;
        let expected_schema = if metadata.requires_response_object() {
            WORKFLOW_CONNECTOR_RESUME_SCHEMA_V2
        } else {
            WORKFLOW_CONNECTOR_RESUME_SCHEMA
        };
        if self.schema != expected_schema
            || self.organization_id != metadata.organization_id
            || self.project_id != metadata.project_id
            || self.workflow_run_id != metadata.workflow_run_id
            || self.step_id != metadata.step_id
            || self.step_attempt != metadata.step_attempt
            || self.observation != metadata.observation
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
        {
            return Err("Workflow Connector resume authority is invalid".into());
        }
        match &self.resolution {
            WorkflowConnectorResumeResolution::Completed { evidence } => {
                evidence.validate_authority(
                    expected_attempt_id,
                    expected_request_digest,
                    expected_request_body_bytes,
                )?;
                if metadata.requires_response_object()
                    != (evidence.schema == WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2)
                {
                    return Err(
                        "Workflow Connector evidence changed its response-object version".into(),
                    );
                }
            }
            WorkflowConnectorResumeResolution::Deferred {
                connector_attempt_id,
                retry_not_before,
            } if *connector_attempt_id == expected_attempt_id
                && *retry_not_before == canonical_timestamp(*retry_not_before) => {}
            WorkflowConnectorResumeResolution::Indeterminate {
                connector_attempt_id,
                dispatch_started_at,
                outcome_deadline_at,
            } if *connector_attempt_id == expected_attempt_id
                && *dispatch_started_at == canonical_timestamp(*dispatch_started_at)
                && *outcome_deadline_at == canonical_timestamp(*outcome_deadline_at)
                && outcome_deadline_at >= dispatch_started_at => {}
            WorkflowConnectorResumeResolution::Rejected { reason } if valid_reason(reason) => {}
            _ => return Err("Workflow Connector resume resolution is invalid".into()),
        }
        if self.compute_digest()? != self.digest {
            return Err("Workflow Connector resume digest does not match".into());
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = WorkflowConnectorResumeDigestContent {
            schema: &self.schema,
            organization_id: self.organization_id,
            project_id: self.project_id,
            workflow_run_id: self.workflow_run_id,
            step_id: &self.step_id,
            step_attempt: self.step_attempt,
            observation: self.observation,
            flow_run_id: &self.flow_run_id,
            flow_hook_id: &self.flow_hook_id,
            resolution: &self.resolution,
        };
        let canonical = canonical_json_bounded(
            &content,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Connector resume digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

fn valid_step_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_reason(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16 * 1024 && !value.contains(['\0', '\r', '\n'])
}

fn zero_digest() -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::{
        WorkflowRunApplicationProjection, WORKFLOW_RUN_FLOW_VERSION_V10,
        WORKFLOW_RUN_INPUT_SCHEMA_V13, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
    };
    use crate::modules::workflow::test_support::{
        cancellation_compensating_connector_workflow_run_input, connector_workflow_run_input,
    };

    fn authority() -> (WorkflowConnectorHookMetadata, Uuid, Sha256Digest, u64) {
        let input = connector_workflow_run_input().expect("Connector WorkflowRun input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "invoke")
            .expect("Connector step");
        let metadata = WorkflowConnectorHookMetadata::from_run_step(
            &input,
            &step,
            input.goal_input.clone(),
            1,
            1,
        )
        .expect("metadata");
        (
            metadata,
            Uuid::now_v7(),
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            42,
        )
    }

    #[test]
    fn hook_metadata_is_attempt_and_observation_bound() {
        let (metadata, ..) = authority();
        assert_eq!(metadata.flow_hook_id(), "workflow-connector:invoke:1:1");
        metadata.validate().expect("valid metadata");

        let mut drifted = metadata.clone();
        drifted.observation = 0;
        assert!(drifted.validate().is_err());
        drifted = metadata;
        drifted.effective_input["ticketId"] = serde_json::json!("changed");
        assert!(drifted.validate().is_err());
    }

    #[test]
    fn application_v10_connector_keeps_typed_response_authority() {
        let mut input = connector_workflow_run_input().expect("Connector WorkflowRun input");
        input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V10.into();
        input.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10.into();
        input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V10.into();
        input.application_projection =
            Some(WorkflowRunApplicationProjection::from_plan(&input.plan).expect("final Output"));
        input.validate().expect("Application Connector input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "invoke")
            .expect("Connector step");
        let metadata = WorkflowConnectorHookMetadata::from_run_step(
            &input,
            &step,
            input.goal_input.clone(),
            1,
            1,
        )
        .expect("metadata");
        assert_eq!(metadata.schema, WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3);
        assert!(metadata.requires_typed_response());
    }

    #[test]
    fn application_v13_connector_keeps_typed_response_authority() {
        let mut input = connector_workflow_run_input().expect("Connector WorkflowRun input");
        input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V13.into();
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "invoke")
            .expect("Connector step");
        let metadata = WorkflowConnectorHookMetadata::from_run_step(
            &input,
            &step,
            input.goal_input.clone(),
            1,
            1,
        )
        .expect("metadata");
        assert_eq!(metadata.schema, WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3);
        assert!(metadata.requires_typed_response());
    }

    #[test]
    fn cancellation_compensation_metadata_has_distinct_purpose_bound_authority() {
        let input = cancellation_compensating_connector_workflow_run_input()
            .expect("cancellation-compensating Connector WorkflowRun input");
        let steps = input.resolved_steps().expect("resolved steps");
        let source = steps
            .iter()
            .find(|step| step.plan.id == "reserve")
            .expect("source step");
        let target = steps
            .iter()
            .find(|step| step.plan.id == "release")
            .expect("compensation step");
        let metadata = WorkflowConnectorHookMetadata::from_cancellation_compensation(
            &input,
            source,
            target,
            serde_json::json!({"reservationId": "resv-0001"}),
            1,
            1,
        )
        .expect("cancellation compensation metadata");

        assert_eq!(metadata.schema, WORKFLOW_CONNECTOR_HOOK_SCHEMA_V4);
        assert_eq!(
            metadata.purpose,
            WorkflowConnectorInvocationPurpose::CancellationCompensation {
                source_step_id: "reserve".into()
            }
        );
        assert_eq!(
            metadata.flow_hook_id(),
            "workflow-connector-cancellation-compensation:reserve:release:1:1"
        );
        assert!(metadata.requires_response_object());
        assert!(metadata.requires_typed_response());
        metadata.validate().expect("valid purpose-bound metadata");

        let encoded = serde_json::to_value(&metadata).expect("encoded metadata");
        assert_eq!(
            encoded["purpose"]["kind"],
            serde_json::json!("cancellation_compensation")
        );
        assert_eq!(encoded["purpose"]["source_step_id"], "reserve");
    }

    #[test]
    fn resume_payload_rejects_evidence_and_digest_tampering() {
        let (metadata, attempt_id, request_digest, request_bytes) = authority();
        let response_digest = Sha256Digest::from_bytes(br#"{"accepted":true}"#);
        let hexadecimal = response_digest
            .as_str()
            .strip_prefix("sha256:")
            .expect("digest");
        let response_object = WorkflowConnectorResponseObjectReference::new(
            attempt_id,
            format!("attempts/{attempt_id}/sha256/{hexadecimal}/body"),
            response_digest.clone(),
            br#"{"accepted":true}"#.len() as u64,
        )
        .expect("response object");
        let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
            attempt_id,
            request_digest.clone(),
            request_bytes,
            WorkflowConnectorAttemptOutcome::Accepted,
            Some(200),
            Some(response_digest),
            Some(br#"{"accepted":true}"#.len() as u64),
            Some(response_object),
            None,
            DateTime::parse_from_rfc3339("2026-08-09T08:00:01Z")
                .expect("time")
                .with_timezone(&Utc),
            DateTime::parse_from_rfc3339("2026-08-09T08:00:02Z")
                .expect("time")
                .with_timezone(&Utc),
        )
        .expect("evidence");
        let payload = WorkflowConnectorResumePayload::completed(
            &metadata,
            evidence,
            attempt_id,
            &request_digest,
            request_bytes,
        )
        .expect("payload");
        payload
            .validate(&metadata, attempt_id, &request_digest, request_bytes)
            .expect("valid payload");

        let mut drifted = payload.clone();
        if let WorkflowConnectorResumeResolution::Completed { evidence } = &mut drifted.resolution {
            evidence.response_status = Some(201);
        }
        assert!(drifted
            .validate(&metadata, attempt_id, &request_digest, request_bytes)
            .is_err());

        let mut drifted = payload;
        drifted.digest = zero_digest().expect("zero digest");
        assert!(drifted
            .validate(&metadata, attempt_id, &request_digest, request_bytes)
            .is_err());
    }

    #[test]
    fn public_connector_contracts_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkflowConnectorHookMetadata>();
        assert_send_sync::<WorkflowConnectorAttemptEvidence>();
        assert_send_sync::<WorkflowConnectorResponseObjectReference>();
        assert_send_sync::<WorkflowConnectorStepOutput>();
        assert_send_sync::<WorkflowConnectorResumePayload>();
    }
}
