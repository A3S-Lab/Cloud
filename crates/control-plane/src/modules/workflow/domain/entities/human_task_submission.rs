use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, AuthorizationDecisionRef,
    FormSubmissionId, HumanTaskId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use a3s_form_core::{
    canonicalize_interaction_value, digest_interaction_value, parse_json, CanonicalValue,
    FormInteractionOutcome, FormInteractionOutputMapping, FormInteractionRequest,
    FormInteractionSubmission, FormReleaseRef, DEFAULT_INTERACTION_MAX_VALUE_BYTES,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const HUMAN_TASK_SUBMISSION_DIGEST_CONTENT_MAX_BYTES: usize = 64 * 1024;
pub const HUMAN_TASK_SUBMISSION_MAX_VALUE_BYTES: u64 = DEFAULT_INTERACTION_MAX_VALUE_BYTES;

#[derive(Debug, Clone, PartialEq)]
pub struct AcceptedHumanTaskSubmission {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: FormSubmissionId,
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub principal_id: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub request: FormInteractionRequest,
    pub submission: FormInteractionSubmission,
    pub accepted_value: CanonicalValue,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanTaskSubmission {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: FormSubmissionId,
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub step_id: String,
    pub step_attempt: u64,
    pub form_release: FormReleaseRef,
    pub assignment_policy_id: String,
    pub assignment_policy_revision: u64,
    pub assignment_policy_digest: Sha256Digest,
    pub task_version: u64,
    pub task_created_at: DateTime<Utc>,
    pub task_due_at: Option<DateTime<Utc>>,
    pub task_expires_at: Option<DateTime<Utc>>,
    pub principal_id: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub outcome: FormInteractionOutcome,
    pub output_mapping: FormInteractionOutputMapping,
    pub request_id: String,
    pub request_digest: Sha256Digest,
    pub interaction_submission_id: String,
    pub idempotency_key: String,
    pub candidate_value_digest: Sha256Digest,
    pub max_value_bytes: u64,
    pub canonical_output: String,
    pub output_digest: Sha256Digest,
    pub digest: Sha256Digest,
    pub aggregate_version: u64,
    pub submitted_at: DateTime<Utc>,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct HumanTaskSubmissionDigestContent<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    id: FormSubmissionId,
    workflow_run_id: WorkflowRunId,
    human_task_id: HumanTaskId,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    step_id: &'a str,
    step_attempt: u64,
    form_release: &'a FormReleaseRef,
    assignment_policy_id: &'a str,
    assignment_policy_revision: u64,
    assignment_policy_digest: &'a Sha256Digest,
    task_version: u64,
    task_created_at: DateTime<Utc>,
    task_due_at: Option<DateTime<Utc>>,
    task_expires_at: Option<DateTime<Utc>>,
    principal_id: PrincipalId,
    authorization_decision: &'a AuthorizationDecisionRef,
    outcome: FormInteractionOutcome,
    output_mapping: &'a FormInteractionOutputMapping,
    request_id: &'a str,
    request_digest: &'a Sha256Digest,
    interaction_submission_id: &'a str,
    idempotency_key: &'a str,
    candidate_value_digest: &'a Sha256Digest,
    max_value_bytes: u64,
    output_digest: &'a Sha256Digest,
    aggregate_version: u64,
    submitted_at: DateTime<Utc>,
    accepted_at: DateTime<Utc>,
}

impl HumanTaskSubmission {
    pub fn accept(input: AcceptedHumanTaskSubmission) -> Result<Self, String> {
        input
            .request
            .validate()
            .map_err(|error| format!("Form interaction request is invalid: {error}"))?;
        input
            .submission
            .validate()
            .map_err(|error| format!("Form interaction submission is invalid: {error}"))?;
        input.form_contract_matches()?;
        input.authorization_decision.validate()?;

        let submitted_at = parse_form_timestamp(&input.submission.submitted_at)?;
        let accepted_at = canonical_timestamp(input.accepted_at);
        let task_created_at = parse_form_timestamp(&input.request.task.created_at)?;
        let task_due_at = input
            .request
            .task
            .due_at
            .as_deref()
            .map(parse_form_timestamp)
            .transpose()?;
        let task_expires_at = input
            .request
            .task
            .expires_at
            .as_deref()
            .map(parse_form_timestamp)
            .transpose()?;
        if submitted_at < task_created_at || accepted_at < submitted_at {
            return Err("Form submission timestamps are inconsistent".into());
        }
        if matches!((task_due_at, task_expires_at), (Some(due), Some(expires)) if due > expires) {
            return Err("Form submission task deadline follows its expiry".into());
        }
        if let Some(expires_at) = task_expires_at {
            if submitted_at >= expires_at || accepted_at >= expires_at {
                return Err("Form submission was accepted after the task expired".into());
            }
        }
        if input.accepted_value.as_object().is_none() {
            return Err("accepted Form output must be a JSON object".into());
        }
        let canonical_output = canonicalize_interaction_value(&input.accepted_value)
            .map_err(|error| format!("accepted Form output is not canonicalizable: {error}"))?;
        if u64::try_from(canonical_output.len()).unwrap_or(u64::MAX) > input.request.max_value_bytes
        {
            return Err("accepted Form output exceeds the request value bound".into());
        }
        let canonical_output = String::from_utf8(canonical_output)
            .map_err(|_| "accepted Form output is not UTF-8".to_owned())?;
        let output_digest = Sha256Digest::parse(
            digest_interaction_value(&input.accepted_value)
                .map_err(|error| format!("accepted Form output cannot be hashed: {error}"))?,
        )?;

        let mut value = Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: input.id,
            workflow_run_id: input.workflow_run_id,
            human_task_id: input.human_task_id,
            flow_run_id: input.submission.identity.flow_run_id.clone(),
            flow_hook_id: input.submission.identity.flow_hook_id.clone(),
            step_id: input.submission.identity.step_id.clone(),
            step_attempt: input.submission.identity.step_attempt,
            form_release: input.submission.form.clone(),
            assignment_policy_id: input.submission.assignment.policy_id.clone(),
            assignment_policy_revision: input.submission.assignment.policy_revision,
            assignment_policy_digest: Sha256Digest::parse(
                input.submission.assignment.policy_digest.clone(),
            )?,
            task_version: input.submission.task_version,
            task_created_at,
            task_due_at,
            task_expires_at,
            principal_id: input.principal_id,
            authorization_decision: input.authorization_decision,
            outcome: input.submission.outcome,
            output_mapping: input.request.output_mapping.clone(),
            request_id: input.submission.request_id.clone(),
            request_digest: Sha256Digest::parse(input.submission.request_digest.clone())?,
            interaction_submission_id: input.submission.submission_id.clone(),
            idempotency_key: input.submission.idempotency_key.clone(),
            candidate_value_digest: Sha256Digest::parse(input.submission.value_digest.clone())?,
            max_value_bytes: input.request.max_value_bytes,
            canonical_output,
            output_digest,
            digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
            aggregate_version: 1,
            submitted_at,
            accepted_at,
        };
        value.digest = value.compute_digest()?;
        value.validate()?;
        Ok(value)
    }

    pub fn accepted_output(&self) -> Result<CanonicalValue, String> {
        let value = parse_json(self.canonical_output.as_bytes())
            .map_err(|error| format!("stored accepted Form output is invalid: {error}"))?;
        if value.as_object().is_none() {
            return Err("stored accepted Form output must be an object".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.human_task_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.aggregate_version != 1
            || self.step_attempt == 0
            || self.task_version == 0
            || self.assignment_policy_revision == 0
            || !valid_external_identity(&self.flow_run_id)
            || !valid_external_identity(&self.flow_hook_id)
            || !valid_external_identity(&self.step_id)
            || !valid_external_identity(&self.assignment_policy_id)
            || !valid_external_identity(&self.request_id)
            || !valid_external_identity(&self.interaction_submission_id)
            || !valid_external_identity(&self.idempotency_key)
            || self.interaction_submission_id != self.id.to_string()
            || self.form_release.organization_id != self.organization_id.to_string()
            || self.form_release.project_id != self.project_id.to_string()
            || self.submitted_at != canonical_timestamp(self.submitted_at)
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.task_created_at != canonical_timestamp(self.task_created_at)
            || self.task_due_at.is_some_and(|due_at| {
                due_at != canonical_timestamp(due_at) || due_at < self.task_created_at
            })
            || self.task_expires_at.is_some_and(|expires_at| {
                expires_at != canonical_timestamp(expires_at) || expires_at < self.task_created_at
            })
            || matches!((self.task_due_at, self.task_expires_at), (Some(due), Some(expires)) if due > expires)
            || self.submitted_at < self.task_created_at
            || self.accepted_at < self.submitted_at
            || self.task_expires_at.is_some_and(|expires_at| {
                self.submitted_at >= expires_at || self.accepted_at >= expires_at
            })
            || !(1..=HUMAN_TASK_SUBMISSION_MAX_VALUE_BYTES).contains(&self.max_value_bytes)
        {
            return Err("stored HumanTaskSubmission identity or timestamps are invalid".into());
        }
        self.form_release
            .validate()
            .map_err(|error| format!("stored FormReleaseRef is invalid: {error}"))?;
        self.authorization_decision.validate()?;
        validate_output_mapping(&self.output_mapping)?;
        let accepted_output = self.accepted_output()?;
        let canonical_output = canonicalize_interaction_value(&accepted_output)
            .map_err(|error| format!("stored accepted Form output is invalid: {error}"))?;
        if u64::try_from(canonical_output.len()).unwrap_or(u64::MAX) > self.max_value_bytes
            || canonical_output.as_slice() != self.canonical_output.as_bytes()
            || digest_interaction_value(&accepted_output)
                .map_err(|error| format!("stored accepted Form output cannot be hashed: {error}"))?
                != self.output_digest.as_str()
            || self.compute_digest()? != self.digest
        {
            return Err("stored HumanTaskSubmission canonical content or digest is invalid".into());
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = HumanTaskSubmissionDigestContent {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            workflow_run_id: self.workflow_run_id,
            human_task_id: self.human_task_id,
            flow_run_id: &self.flow_run_id,
            flow_hook_id: &self.flow_hook_id,
            step_id: &self.step_id,
            step_attempt: self.step_attempt,
            form_release: &self.form_release,
            assignment_policy_id: &self.assignment_policy_id,
            assignment_policy_revision: self.assignment_policy_revision,
            assignment_policy_digest: &self.assignment_policy_digest,
            task_version: self.task_version,
            task_created_at: self.task_created_at,
            task_due_at: self.task_due_at,
            task_expires_at: self.task_expires_at,
            principal_id: self.principal_id,
            authorization_decision: &self.authorization_decision,
            outcome: self.outcome,
            output_mapping: &self.output_mapping,
            request_id: &self.request_id,
            request_digest: &self.request_digest,
            interaction_submission_id: &self.interaction_submission_id,
            idempotency_key: &self.idempotency_key,
            candidate_value_digest: &self.candidate_value_digest,
            max_value_bytes: self.max_value_bytes,
            output_digest: &self.output_digest,
            aggregate_version: self.aggregate_version,
            submitted_at: self.submitted_at,
            accepted_at: self.accepted_at,
        };
        let canonical = canonical_json_bounded(
            &content,
            HUMAN_TASK_SUBMISSION_DIGEST_CONTENT_MAX_BYTES,
            "HumanTaskSubmission digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

impl AcceptedHumanTaskSubmission {
    fn form_contract_matches(&self) -> Result<(), String> {
        let request = &self.request;
        let submission = &self.submission;
        if request.request_id != submission.request_id
            || request.digest != submission.request_digest
            || request.identity != submission.identity
            || request.form != submission.form
            || request.assignment.policy_id != submission.assignment.policy_id
            || request.assignment.policy_revision != submission.assignment.policy_revision
            || request.assignment.policy_digest != submission.assignment.policy_digest
            || request.assignment.claimed_principal_id != submission.principal_id
            || request.task.version != submission.task_version
            || !request.allowed_outcomes.contains(&submission.outcome)
            || request.form.organization_id != self.organization_id.to_string()
            || request.form.project_id != self.project_id.to_string()
            || submission.submission_id != self.id.to_string()
            || submission.identity.workflow_run_id != self.workflow_run_id.to_string()
            || submission.identity.human_task_id != self.human_task_id.to_string()
            || submission.principal_id != self.principal_id.to_string()
        {
            return Err(
                "Form submission does not match its request or Cloud authority bindings".into(),
            );
        }
        Ok(())
    }
}

fn parse_form_timestamp(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| format!("Form interaction timestamp is invalid: {error}"))
}

fn validate_output_mapping(mapping: &FormInteractionOutputMapping) -> Result<(), String> {
    if let FormInteractionOutputMapping::Registry {
        registry_key,
        revision,
        digest,
    } = mapping
    {
        if !valid_external_identity(registry_key) || *revision == 0 {
            return Err("stored HumanTaskSubmission output mapping is invalid".into());
        }
        Sha256Digest::parse(digest.clone())?;
    }
    Ok(())
}

fn valid_external_identity(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= 512
        && !value.contains(['\0', '\r', '\n'])
}
