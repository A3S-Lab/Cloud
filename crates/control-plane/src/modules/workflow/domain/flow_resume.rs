use super::{WorkflowDecision, WorkflowDecisionOutcome};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, FormSubmissionId, HumanTaskId,
    Sha256Digest, WorkflowDecisionId, WorkflowRunId,
};
use a3s_form_core::{canonicalize_interaction_value, digest_interaction_value, CanonicalValue};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const FLOW_RESUME_PAYLOAD_API_VERSION: &str = "a3s.dev/flow-resume-payload/v1";
pub const FLOW_RESUME_RECEIPT_API_VERSION: &str = "a3s.dev/flow-resume-receipt/v1";
const MAX_FLOW_RESUME_PAYLOAD_BYTES: usize = 1_100_000;
const MAX_EXTERNAL_IDENTITY_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowResumePayload {
    pub api_version: String,
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub workflow_decision_id: WorkflowDecisionId,
    pub decision_digest: Sha256Digest,
    pub outcome: WorkflowDecisionOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_submission_id: Option<FormSubmissionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_submission_digest: Option<Sha256Digest>,
    pub output: CanonicalValue,
    pub output_digest: Sha256Digest,
    pub digest: Sha256Digest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowResumePayloadDigestContent<'a> {
    api_version: &'a str,
    workflow_run_id: WorkflowRunId,
    human_task_id: HumanTaskId,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    workflow_decision_id: WorkflowDecisionId,
    decision_digest: &'a Sha256Digest,
    outcome: WorkflowDecisionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    form_submission_id: Option<FormSubmissionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    form_submission_digest: Option<&'a Sha256Digest>,
    output: &'a CanonicalValue,
    output_digest: &'a Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowResumeReceipt {
    pub api_version: String,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub workflow_decision_id: WorkflowDecisionId,
    pub payload_digest: Sha256Digest,
    pub hook_event_sequence: u64,
    pub hook_event_id: Uuid,
    pub hook_received_at: DateTime<Utc>,
}

impl FlowResumePayload {
    pub fn from_decision(decision: &WorkflowDecision) -> Result<Self, String> {
        decision.validate()?;
        let mut value = Self {
            api_version: FLOW_RESUME_PAYLOAD_API_VERSION.into(),
            workflow_run_id: decision.workflow_run_id,
            human_task_id: decision.human_task_id,
            flow_run_id: decision.flow_run_id.clone(),
            flow_hook_id: decision.flow_hook_id.clone(),
            workflow_decision_id: decision.id,
            decision_digest: decision.digest.clone(),
            outcome: decision.outcome,
            form_submission_id: decision.form_submission_id,
            form_submission_digest: decision.form_submission_digest.clone(),
            output: decision.output()?,
            output_digest: decision.output_digest.clone(),
            digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
        };
        value.digest = value.compute_digest()?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.api_version != FLOW_RESUME_PAYLOAD_API_VERSION
            || self.workflow_run_id.as_uuid().is_nil()
            || self.human_task_id.as_uuid().is_nil()
            || self.workflow_decision_id.as_uuid().is_nil()
            || !valid_external_identity(&self.flow_run_id)
            || !valid_external_identity(&self.flow_hook_id)
            || self.outcome.is_interactive()
                != (self.form_submission_id.is_some() && self.form_submission_digest.is_some())
            || self.output.as_object().is_none()
        {
            return Err("Flow resume payload identity or outcome binding is invalid".into());
        }
        let canonical_output = canonicalize_interaction_value(&self.output)
            .map_err(|error| format!("Flow resume output is invalid: {error}"))?;
        if digest_interaction_value(&self.output)
            .map_err(|error| format!("Flow resume output cannot be hashed: {error}"))?
            != self.output_digest.as_str()
            || canonical_output.len() > MAX_FLOW_RESUME_PAYLOAD_BYTES
            || self.compute_digest()? != self.digest
        {
            return Err("Flow resume payload output or digest is invalid".into());
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("Flow resume payload cannot be encoded: {error}"))?;
        if encoded.len() > MAX_FLOW_RESUME_PAYLOAD_BYTES {
            return Err("Flow resume payload exceeds its byte bound".into());
        }
        Ok(())
    }

    pub fn to_flow_value(&self) -> Result<serde_json::Value, String> {
        self.validate()?;
        serde_json::to_value(self)
            .map_err(|error| format!("Flow resume payload cannot be projected: {error}"))
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = FlowResumePayloadDigestContent {
            api_version: &self.api_version,
            workflow_run_id: self.workflow_run_id,
            human_task_id: self.human_task_id,
            flow_run_id: &self.flow_run_id,
            flow_hook_id: &self.flow_hook_id,
            workflow_decision_id: self.workflow_decision_id,
            decision_digest: &self.decision_digest,
            outcome: self.outcome,
            form_submission_id: self.form_submission_id,
            form_submission_digest: self.form_submission_digest.as_ref(),
            output: &self.output,
            output_digest: &self.output_digest,
        };
        let canonical = canonical_json_bounded(
            &content,
            MAX_FLOW_RESUME_PAYLOAD_BYTES,
            "Flow resume payload digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

impl FlowResumeReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn from_hook_received(
        payload: &FlowResumePayload,
        flow_run_id: &str,
        flow_hook_id: &str,
        observed_payload: &serde_json::Value,
        hook_event_sequence: u64,
        hook_event_id: Uuid,
        hook_received_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        payload.validate()?;
        let expected_payload = payload.to_flow_value()?;
        if flow_run_id != payload.flow_run_id
            || flow_hook_id != payload.flow_hook_id
            || observed_payload != &expected_payload
            || hook_event_sequence == 0
            || hook_event_id.is_nil()
        {
            return Err("Flow HookReceived event does not match the resume payload".into());
        }
        let receipt = Self {
            api_version: FLOW_RESUME_RECEIPT_API_VERSION.into(),
            flow_run_id: flow_run_id.into(),
            flow_hook_id: flow_hook_id.into(),
            workflow_decision_id: payload.workflow_decision_id,
            payload_digest: payload.digest.clone(),
            hook_event_sequence,
            hook_event_id,
            hook_received_at: canonical_timestamp(hook_received_at),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.api_version != FLOW_RESUME_RECEIPT_API_VERSION
            || !valid_external_identity(&self.flow_run_id)
            || !valid_external_identity(&self.flow_hook_id)
            || self.workflow_decision_id.as_uuid().is_nil()
            || self.hook_event_sequence == 0
            || self.hook_event_id.is_nil()
            || self.hook_received_at != canonical_timestamp(self.hook_received_at)
        {
            return Err("Flow resume receipt is invalid".into());
        }
        Ok(())
    }
}

fn valid_external_identity(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_EXTERNAL_IDENTITY_BYTES
        && !value.contains(['\0', '\r', '\n'])
}
