use super::{HumanTask, HumanTaskStatus, HumanTaskSubmission};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, AuthorizationDecisionRef,
    FormSubmissionId, HumanTaskId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDecisionId, WorkflowRunId,
};
use crate::modules::workflow::domain::AssignmentPolicyRef;
use a3s_form_core::{
    canonicalize_interaction_value, digest_interaction_value, parse_json, CanonicalValue,
    FormInteractionOutcome, FormInteractionOutputMapping, FormReleaseRef,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_DECISION_OUTPUT_BYTES: usize = 1_000_000;
const WORKFLOW_DECISION_DIGEST_CONTENT_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDecisionOutcome {
    Submit,
    Approve,
    Reject,
    Expire,
    Cancel,
}

impl WorkflowDecisionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Expire => "expire",
            Self::Cancel => "cancel",
        }
    }

    pub const fn is_interactive(self) -> bool {
        matches!(self, Self::Submit | Self::Approve | Self::Reject)
    }
}

impl From<FormInteractionOutcome> for WorkflowDecisionOutcome {
    fn from(value: FormInteractionOutcome) -> Self {
        match value {
            FormInteractionOutcome::Submit => Self::Submit,
            FormInteractionOutcome::Approve => Self::Approve,
            FormInteractionOutcome::Reject => Self::Reject,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDecision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: WorkflowDecisionId,
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub step_id: String,
    pub step_attempt: u64,
    pub task_version: u64,
    pub form_release: FormReleaseRef,
    pub assignment_policy: AssignmentPolicyRef,
    pub outcome: WorkflowDecisionOutcome,
    pub form_submission_id: Option<FormSubmissionId>,
    pub form_submission_digest: Option<Sha256Digest>,
    pub decided_by: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub canonical_output: String,
    pub output_digest: Sha256Digest,
    pub digest: Sha256Digest,
    pub decided_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct WorkflowDecisionDigestContent<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    id: WorkflowDecisionId,
    workflow_run_id: WorkflowRunId,
    human_task_id: HumanTaskId,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    step_id: &'a str,
    step_attempt: u64,
    task_version: u64,
    form_release: &'a FormReleaseRef,
    assignment_policy: &'a AssignmentPolicyRef,
    outcome: WorkflowDecisionOutcome,
    form_submission_id: Option<FormSubmissionId>,
    form_submission_digest: Option<&'a Sha256Digest>,
    decided_by: PrincipalId,
    authorization_decision: &'a AuthorizationDecisionRef,
    output_digest: &'a Sha256Digest,
    decided_at: DateTime<Utc>,
}

impl WorkflowDecision {
    pub fn from_submission(
        id: WorkflowDecisionId,
        task: &HumanTask,
        submission: &HumanTaskSubmission,
        output: CanonicalValue,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        task.validate()?;
        submission.validate()?;
        if task.status != HumanTaskStatus::Claimed
            || task.claimed_by != Some(submission.principal_id)
            || submission.organization_id != task.organization_id
            || submission.project_id != task.project_id
            || submission.workflow_run_id != task.workflow_run_id
            || submission.human_task_id != task.id
            || submission.flow_run_id != task.flow_run_id
            || submission.flow_hook_id != task.flow_hook_id
            || submission.step_id != task.step_id
            || submission.step_attempt != task.step_attempt
            || submission.task_version != task.aggregate_version
            || submission.form_release != task.form_release
            || submission.assignment_policy_id != task.assignment_policy.id
            || submission.assignment_policy_revision != task.assignment_policy.revision
            || submission.assignment_policy_digest != task.assignment_policy.digest
        {
            return Err("HumanTaskSubmission does not match the claimed HumanTask".into());
        }
        let decided_at = canonical_timestamp(decided_at);
        if decided_at < submission.accepted_at
            || task
                .expires_at
                .is_some_and(|expires_at| decided_at >= expires_at)
        {
            return Err("interactive WorkflowDecision time is invalid or expired".into());
        }
        let (canonical_output, output_digest) = canonical_output(&output)?;
        if matches!(
            submission.output_mapping,
            FormInteractionOutputMapping::Identity
        ) && canonical_output != submission.canonical_output
        {
            return Err("identity output mapping changed the accepted Form output".into());
        }
        Self::build(
            id,
            task,
            WorkflowDecisionOutcome::from(submission.outcome),
            Some(submission.id),
            Some(submission.digest.clone()),
            submission.principal_id,
            submission.authorization_decision.clone(),
            canonical_output,
            output_digest,
            decided_at,
        )
    }

    pub fn expire(
        id: WorkflowDecisionId,
        task: &HumanTask,
        decided_by: PrincipalId,
        authorization_decision: AuthorizationDecisionRef,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let decided_at = canonical_timestamp(decided_at);
        let expires_at = task
            .expires_at
            .ok_or_else(|| "HumanTask has no expiry deadline".to_owned())?;
        if decided_at < expires_at {
            return Err("WorkflowDecision cannot expire a HumanTask before its deadline".into());
        }
        Self::terminal(
            id,
            task,
            WorkflowDecisionOutcome::Expire,
            decided_by,
            authorization_decision,
            decided_at,
        )
    }

    pub fn cancel(
        id: WorkflowDecisionId,
        task: &HumanTask,
        decided_by: PrincipalId,
        authorization_decision: AuthorizationDecisionRef,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::terminal(
            id,
            task,
            WorkflowDecisionOutcome::Cancel,
            decided_by,
            authorization_decision,
            canonical_timestamp(decided_at),
        )
    }

    pub fn output(&self) -> Result<CanonicalValue, String> {
        let output = parse_json(self.canonical_output.as_bytes())
            .map_err(|error| format!("stored WorkflowDecision output is invalid: {error}"))?;
        if output.as_object().is_none() {
            return Err("stored WorkflowDecision output must be an object".into());
        }
        Ok(output)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.human_task_id.as_uuid().is_nil()
            || self.decided_by.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.task_version == 0
            || !valid_external_identity(&self.flow_run_id)
            || !valid_external_identity(&self.flow_hook_id)
            || !valid_external_identity(&self.step_id)
            || self
                .form_submission_id
                .is_some_and(|submission_id| submission_id.as_uuid().is_nil())
            || self.form_release.organization_id != self.organization_id.to_string()
            || self.form_release.project_id != self.project_id.to_string()
            || self.decided_at != canonical_timestamp(self.decided_at)
        {
            return Err("stored WorkflowDecision identity or timestamp is invalid".into());
        }
        self.form_release
            .validate()
            .map_err(|error| format!("WorkflowDecision FormReleaseRef is invalid: {error}"))?;
        self.assignment_policy.validate()?;
        self.authorization_decision.validate()?;
        let interactive_references =
            self.form_submission_id.is_some() && self.form_submission_digest.is_some();
        if self.outcome.is_interactive() != interactive_references {
            return Err("WorkflowDecision submission references do not match its outcome".into());
        }
        let output = self.output()?;
        let canonical = canonicalize_interaction_value(&output)
            .map_err(|error| format!("stored WorkflowDecision output is invalid: {error}"))?;
        if canonical.len() > MAX_DECISION_OUTPUT_BYTES
            || canonical.as_slice() != self.canonical_output.as_bytes()
            || digest_interaction_value(&output).map_err(|error| {
                format!("stored WorkflowDecision output cannot be hashed: {error}")
            })? != self.output_digest.as_str()
            || self.compute_digest()? != self.digest
        {
            return Err("stored WorkflowDecision output or digest is invalid".into());
        }
        Ok(())
    }

    pub(super) fn validate_for_task(&self, task: &HumanTask) -> Result<(), String> {
        self.validate()?;
        if self.organization_id != task.organization_id
            || self.project_id != task.project_id
            || self.workflow_run_id != task.workflow_run_id
            || self.human_task_id != task.id
            || self.flow_run_id != task.flow_run_id
            || self.flow_hook_id != task.flow_hook_id
            || self.step_id != task.step_id
            || self.step_attempt != task.step_attempt
            || self.task_version != task.aggregate_version
            || self.form_release != task.form_release
            || self.assignment_policy != task.assignment_policy
            || self.decided_at < task.updated_at
        {
            return Err("WorkflowDecision does not match its HumanTask generation".into());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        id: WorkflowDecisionId,
        task: &HumanTask,
        outcome: WorkflowDecisionOutcome,
        form_submission_id: Option<FormSubmissionId>,
        form_submission_digest: Option<Sha256Digest>,
        decided_by: PrincipalId,
        authorization_decision: AuthorizationDecisionRef,
        canonical_output: String,
        output_digest: Sha256Digest,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if task.status.is_terminal() {
            return Err("terminal HumanTask cannot create another WorkflowDecision".into());
        }
        task.validate()?;
        authorization_decision.validate()?;
        let mut value = Self {
            organization_id: task.organization_id,
            project_id: task.project_id,
            id,
            workflow_run_id: task.workflow_run_id,
            human_task_id: task.id,
            flow_run_id: task.flow_run_id.clone(),
            flow_hook_id: task.flow_hook_id.clone(),
            step_id: task.step_id.clone(),
            step_attempt: task.step_attempt,
            task_version: task.aggregate_version,
            form_release: task.form_release.clone(),
            assignment_policy: task.assignment_policy.clone(),
            outcome,
            form_submission_id,
            form_submission_digest,
            decided_by,
            authorization_decision,
            canonical_output,
            output_digest,
            digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
            decided_at,
        };
        value.digest = value.compute_digest()?;
        value.validate_for_task(task)?;
        Ok(value)
    }

    fn terminal(
        id: WorkflowDecisionId,
        task: &HumanTask,
        outcome: WorkflowDecisionOutcome,
        decided_by: PrincipalId,
        authorization_decision: AuthorizationDecisionRef,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if outcome.is_interactive() || decided_at < task.updated_at {
            return Err("terminal WorkflowDecision outcome or time is invalid".into());
        }
        let output = parse_json(format!(r#"{{"outcome":"{}"}}"#, outcome.as_str()).as_bytes())
            .map_err(|error| format!("terminal WorkflowDecision output is invalid: {error}"))?;
        let (canonical_output, output_digest) = canonical_output(&output)?;
        Self::build(
            id,
            task,
            outcome,
            None,
            None,
            decided_by,
            authorization_decision,
            canonical_output,
            output_digest,
            decided_at,
        )
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = WorkflowDecisionDigestContent {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            workflow_run_id: self.workflow_run_id,
            human_task_id: self.human_task_id,
            flow_run_id: &self.flow_run_id,
            flow_hook_id: &self.flow_hook_id,
            step_id: &self.step_id,
            step_attempt: self.step_attempt,
            task_version: self.task_version,
            form_release: &self.form_release,
            assignment_policy: &self.assignment_policy,
            outcome: self.outcome,
            form_submission_id: self.form_submission_id,
            form_submission_digest: self.form_submission_digest.as_ref(),
            decided_by: self.decided_by,
            authorization_decision: &self.authorization_decision,
            output_digest: &self.output_digest,
            decided_at: self.decided_at,
        };
        let canonical = canonical_json_bounded(
            &content,
            WORKFLOW_DECISION_DIGEST_CONTENT_MAX_BYTES,
            "WorkflowDecision digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

fn canonical_output(value: &CanonicalValue) -> Result<(String, Sha256Digest), String> {
    if value.as_object().is_none() {
        return Err("WorkflowDecision output must be an object".into());
    }
    let canonical = canonicalize_interaction_value(value)
        .map_err(|error| format!("WorkflowDecision output is invalid: {error}"))?;
    if canonical.len() > MAX_DECISION_OUTPUT_BYTES {
        return Err("WorkflowDecision output exceeds its byte bound".into());
    }
    let canonical = String::from_utf8(canonical)
        .map_err(|_| "WorkflowDecision output is not UTF-8".to_owned())?;
    let digest = Sha256Digest::parse(
        digest_interaction_value(value)
            .map_err(|error| format!("WorkflowDecision output cannot be hashed: {error}"))?,
    )?;
    Ok((canonical, digest))
}

fn valid_external_identity(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= 512
        && !value.contains(['\0', '\r', '\n'])
}
