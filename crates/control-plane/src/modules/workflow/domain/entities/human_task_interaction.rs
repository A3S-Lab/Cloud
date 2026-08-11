use super::{HumanTask, HumanTaskStatus, WorkflowDecision};
use crate::modules::shared_kernel::domain::{PrincipalId, Sha256Digest};
use a3s_form_core::{
    canonicalize_interaction_value, digest_interaction_request, CanonicalValue,
    FormInteractionAssignment, FormInteractionOutcome, FormInteractionOutputMapping,
    FormInteractionRequest, FormInteractionTaskBinding, WorkflowInteractionIdentity,
    DEFAULT_INTERACTION_MAX_VALUE_BYTES, FORM_INTERACTION_REQUEST_API_VERSION,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_INTERACTION_MESSAGE_BYTES: usize = 4_096;
const MAX_INTERACTION_DETAILS_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HumanTaskInteractionSpec {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub allowed_outcomes: Vec<FormInteractionOutcome>,
    pub output_mapping: FormInteractionOutputMapping,
    pub max_value_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_value: Option<CanonicalValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HumanTaskRecord {
    pub task: HumanTask,
    pub interaction: HumanTaskInteractionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_request: Option<FormInteractionRequest>,
    pub hook_event_sequence: u64,
    pub hook_event_id: Uuid,
}

impl HumanTaskInteractionSpec {
    pub fn approval(
        message: impl Into<String>,
        details: Option<String>,
        initial_value: Option<CanonicalValue>,
    ) -> Result<Self, String> {
        let value = Self {
            message: message.into(),
            details,
            allowed_outcomes: vec![
                FormInteractionOutcome::Approve,
                FormInteractionOutcome::Reject,
            ],
            output_mapping: FormInteractionOutputMapping::Identity,
            max_value_bytes: DEFAULT_INTERACTION_MAX_VALUE_BYTES,
            initial_value,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(
            "HumanTask interaction message",
            &self.message,
            1,
            MAX_INTERACTION_MESSAGE_BYTES,
        )?;
        if let Some(details) = &self.details {
            validate_text(
                "HumanTask interaction details",
                details,
                0,
                MAX_INTERACTION_DETAILS_BYTES,
            )?;
        }
        if self.allowed_outcomes.is_empty() || self.allowed_outcomes.len() > 3 {
            return Err("HumanTask interaction outcomes are empty or unbounded".into());
        }
        let unique = self
            .allowed_outcomes
            .iter()
            .map(|outcome| format!("{outcome:?}"))
            .collect::<BTreeSet<_>>();
        if unique.len() != self.allowed_outcomes.len() {
            return Err("HumanTask interaction outcomes contain duplicates".into());
        }
        if self.max_value_bytes == 0 || self.max_value_bytes > DEFAULT_INTERACTION_MAX_VALUE_BYTES {
            return Err("HumanTask interaction value bound is invalid".into());
        }
        if let Some(initial_value) = &self.initial_value {
            if initial_value.as_object().is_none() {
                return Err("HumanTask interaction initial value must be an object".into());
            }
            let canonical = canonicalize_interaction_value(initial_value).map_err(|error| {
                format!("HumanTask interaction initial value is invalid: {error}")
            })?;
            if u64::try_from(canonical.len()).unwrap_or(u64::MAX) > self.max_value_bytes {
                return Err("HumanTask interaction initial value exceeds its byte bound".into());
            }
        }
        validate_output_mapping(&self.output_mapping)
    }

    pub fn request_for_claimed_task(
        &self,
        task: &HumanTask,
        principal_id: PrincipalId,
    ) -> Result<FormInteractionRequest, String> {
        if task.status != HumanTaskStatus::Claimed
            || task.claimed_by != Some(principal_id)
            || task.aggregate_version == 0
        {
            return Err("Form interaction request requires the current HumanTask claimant".into());
        }
        self.request(task, principal_id, task.aggregate_version)
    }

    fn request(
        &self,
        task: &HumanTask,
        principal_id: PrincipalId,
        task_version: u64,
    ) -> Result<FormInteractionRequest, String> {
        self.validate()?;
        task.validate()?;
        let mut request = FormInteractionRequest {
            api_version: FORM_INTERACTION_REQUEST_API_VERSION.into(),
            request_id: interaction_request_id(task, task_version),
            identity: WorkflowInteractionIdentity {
                workflow_run_id: task.workflow_run_id.to_string(),
                flow_run_id: task.flow_run_id.clone(),
                step_id: task.step_id.clone(),
                step_attempt: task.step_attempt,
                human_task_id: task.id.to_string(),
                flow_hook_id: task.flow_hook_id.clone(),
            },
            form: task.form_release.clone(),
            assignment: FormInteractionAssignment {
                policy_id: task.assignment_policy.id.clone(),
                policy_revision: task.assignment_policy.revision,
                policy_digest: task.assignment_policy.digest.to_string(),
                claimed_principal_id: principal_id.to_string(),
            },
            task: FormInteractionTaskBinding {
                version: task_version,
                created_at: form_timestamp(task.created_at),
                due_at: task.due_at.map(form_timestamp),
                expires_at: task.expires_at.map(form_timestamp),
            },
            allowed_outcomes: self.allowed_outcomes.clone(),
            output_mapping: self.output_mapping.clone(),
            max_value_bytes: self.max_value_bytes,
            initial_value: self.initial_value.clone(),
            digest: format!("sha256:{}", "0".repeat(64)),
        };
        request.digest = digest_interaction_request(&request)
            .map_err(|error| format!("Form interaction request cannot be hashed: {error}"))?;
        request
            .validate()
            .map_err(|error| format!("Form interaction request is invalid: {error}"))?;
        Ok(request)
    }

    fn validate_request(
        &self,
        task: &HumanTask,
        request: &FormInteractionRequest,
    ) -> Result<(), String> {
        request
            .validate()
            .map_err(|error| format!("stored Form interaction request is invalid: {error}"))?;
        let principal_id = task
            .claimed_by
            .ok_or_else(|| "stored Form interaction request has no task claimant".to_owned())?;
        let expected_version = if task.status.is_terminal() {
            task.aggregate_version
                .checked_sub(1)
                .ok_or_else(|| "terminal HumanTask version is invalid".to_owned())?
        } else {
            task.aggregate_version
        };
        let expected = self.request(task, principal_id, expected_version)?;
        if request != &expected {
            return Err("stored Form interaction request drifted from its HumanTask".into());
        }
        Ok(())
    }
}

impl HumanTaskRecord {
    pub fn create(
        task: HumanTask,
        interaction: HumanTaskInteractionSpec,
        hook_event_sequence: u64,
        hook_event_id: Uuid,
    ) -> Result<Self, String> {
        let value = Self {
            task,
            interaction,
            interaction_request: None,
            hook_event_sequence,
            hook_event_id,
        };
        if value.task.status != HumanTaskStatus::PendingActivation {
            return Err("new HumanTask record must be pending activation".into());
        }
        value.validate()?;
        Ok(value)
    }

    pub fn activate(
        &mut self,
        expected_version: u64,
        activated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.task.activate(expected_version, activated_at)?;
        self.validate()
    }

    pub fn claim(
        &mut self,
        expected_version: u64,
        principal_id: PrincipalId,
        claimed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.task
            .claim(expected_version, principal_id, claimed_at)?;
        self.interaction_request = Some(
            self.interaction
                .request_for_claimed_task(&self.task, principal_id)?,
        );
        self.validate()
    }

    pub fn release(
        &mut self,
        expected_version: u64,
        principal_id: PrincipalId,
        released_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.task
            .release(expected_version, principal_id, released_at)?;
        self.interaction_request = None;
        self.validate()
    }

    pub fn complete(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        self.task.complete(expected_version, decision)?;
        self.validate()
    }

    pub fn expire(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        self.task.expire(expected_version, decision)?;
        self.validate()
    }

    pub fn cancel(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        self.task.cancel(expected_version, decision)?;
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.task.validate()?;
        self.interaction.validate()?;
        if self.hook_event_sequence == 0 || self.hook_event_id.is_nil() {
            return Err("HumanTask hook evidence is invalid".into());
        }
        match (&self.interaction_request, self.task.claimed_by) {
            (None, None) => {}
            (Some(request), Some(_)) => {
                self.interaction.validate_request(&self.task, request)?;
            }
            _ => return Err("HumanTask claimant and Form interaction request diverged".into()),
        }
        if matches!(
            self.task.status,
            HumanTaskStatus::Claimed | HumanTaskStatus::Completed
        ) && self.interaction_request.is_none()
        {
            return Err("claimed HumanTask is missing its Form interaction request".into());
        }
        Ok(())
    }
}

fn interaction_request_id(task: &HumanTask, task_version: u64) -> String {
    format!("human-task:{}:v{task_version}", task.id)
}

fn form_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn validate_output_mapping(mapping: &FormInteractionOutputMapping) -> Result<(), String> {
    if let FormInteractionOutputMapping::Registry {
        registry_key,
        revision,
        digest,
    } = mapping
    {
        if registry_key.is_empty()
            || registry_key.trim() != registry_key
            || registry_key.len() > 512
            || registry_key.contains(['\0', '\r', '\n'])
            || *revision == 0
            || Sha256Digest::parse(digest.clone()).is_err()
        {
            return Err("HumanTask interaction output mapping is invalid".into());
        }
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), String> {
    if value.len() < minimum
        || value.len() > maximum
        || value.contains('\0')
        || value.trim() != value
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}
