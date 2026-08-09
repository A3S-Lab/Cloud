use super::{WorkflowDecision, WorkflowDecisionOutcome};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, HumanTaskId, OrganizationId, PrincipalId, ProjectId, WorkflowDecisionId,
    WorkflowRunId,
};
use crate::modules::workflow::domain::AssignmentPolicyRef;
use a3s_form_core::FormReleaseRef;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_EXTERNAL_IDENTITY_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanTaskStatus {
    PendingActivation,
    Ready,
    Claimed,
    Completed,
    Expired,
    Cancelled,
}

impl HumanTaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingActivation => "pending_activation",
            Self::Ready => "ready",
            Self::Claimed => "claimed",
            Self::Completed => "completed",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Expired | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHumanTask {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: HumanTaskId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u64,
    pub form_release: FormReleaseRef,
    pub assignment_policy: AssignmentPolicyRef,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub due_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanTask {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: HumanTaskId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u64,
    pub form_release: FormReleaseRef,
    pub assignment_policy: AssignmentPolicyRef,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub status: HumanTaskStatus,
    pub claimed_by: Option<PrincipalId>,
    pub decision_id: Option<WorkflowDecisionId>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub due_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub terminal_at: Option<DateTime<Utc>>,
}

impl HumanTask {
    pub fn create(input: NewHumanTask) -> Result<Self, String> {
        let created_at = canonical_timestamp(input.created_at);
        let value = Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: input.id,
            workflow_run_id: input.workflow_run_id,
            step_id: input.step_id,
            step_attempt: input.step_attempt,
            form_release: input.form_release,
            assignment_policy: input.assignment_policy,
            flow_run_id: input.flow_run_id,
            flow_hook_id: input.flow_hook_id,
            status: HumanTaskStatus::PendingActivation,
            claimed_by: None,
            decision_id: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            due_at: input.due_at.map(canonical_timestamp),
            expires_at: input.expires_at.map(canonical_timestamp),
            claimed_at: None,
            terminal_at: None,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn activate(
        &mut self,
        expected_version: u64,
        activated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != HumanTaskStatus::PendingActivation {
            return Err("only a pending HumanTask can be activated".into());
        }
        let mut next = self.next_version(expected_version, activated_at)?;
        next.ensure_not_expired(next.updated_at)?;
        next.status = HumanTaskStatus::Ready;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn claim(
        &mut self,
        expected_version: u64,
        principal_id: PrincipalId,
        claimed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != HumanTaskStatus::Ready || principal_id.as_uuid().is_nil() {
            return Err("only a ready HumanTask can be claimed by a valid principal".into());
        }
        let mut next = self.next_version(expected_version, claimed_at)?;
        next.ensure_not_expired(next.updated_at)?;
        next.status = HumanTaskStatus::Claimed;
        next.claimed_by = Some(principal_id);
        next.claimed_at = Some(next.updated_at);
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn release(
        &mut self,
        expected_version: u64,
        principal_id: PrincipalId,
        released_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != HumanTaskStatus::Claimed || self.claimed_by != Some(principal_id) {
            return Err("only the current claimant can release a HumanTask".into());
        }
        let mut next = self.next_version(expected_version, released_at)?;
        next.ensure_not_expired(next.updated_at)?;
        next.status = HumanTaskStatus::Ready;
        next.claimed_by = None;
        next.claimed_at = None;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn complete(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        if self.status != HumanTaskStatus::Claimed
            || !decision.outcome.is_interactive()
            || self.claimed_by != Some(decision.decided_by)
        {
            return Err("interactive HumanTask completion requires its current claimant".into());
        }
        self.apply_terminal_decision(expected_version, decision, HumanTaskStatus::Completed)
    }

    pub fn expire(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        if decision.outcome != WorkflowDecisionOutcome::Expire {
            return Err("HumanTask expiry requires an expiry decision".into());
        }
        let expires_at = self
            .expires_at
            .ok_or_else(|| "HumanTask has no expiry deadline".to_owned())?;
        if decision.decided_at < expires_at {
            return Err("HumanTask cannot expire before its expiry deadline".into());
        }
        self.apply_terminal_decision(expected_version, decision, HumanTaskStatus::Expired)
    }

    pub fn cancel(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
    ) -> Result<(), String> {
        if decision.outcome != WorkflowDecisionOutcome::Cancel {
            return Err("HumanTask cancellation requires a cancellation decision".into());
        }
        self.apply_terminal_decision(expected_version, decision, HumanTaskStatus::Cancelled)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.aggregate_version == 0
            || !valid_external_identity(&self.step_id)
            || !valid_external_identity(&self.flow_run_id)
            || !valid_external_identity(&self.flow_hook_id)
            || self.form_release.organization_id != self.organization_id.to_string()
            || self.form_release.project_id != self.project_id.to_string()
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self.due_at.is_some_and(|due_at| {
                due_at != canonical_timestamp(due_at) || due_at < self.created_at
            })
            || self.expires_at.is_some_and(|expires_at| {
                expires_at != canonical_timestamp(expires_at) || expires_at < self.created_at
            })
            || matches!((self.due_at, self.expires_at), (Some(due), Some(expires)) if due > expires)
            || self
                .claimed_by
                .is_some_and(|principal_id| principal_id.as_uuid().is_nil())
            || self
                .decision_id
                .is_some_and(|decision_id| decision_id.as_uuid().is_nil())
        {
            return Err("stored HumanTask identity, deadline, or version is invalid".into());
        }
        self.form_release
            .validate()
            .map_err(|error| format!("HumanTask FormReleaseRef is invalid: {error}"))?;
        self.assignment_policy.validate()?;
        if self.claimed_at.is_some_and(|claimed_at| {
            claimed_at != canonical_timestamp(claimed_at)
                || claimed_at < self.created_at
                || claimed_at > self.updated_at
        }) || self.terminal_at.is_some_and(|terminal_at| {
            terminal_at != canonical_timestamp(terminal_at) || terminal_at != self.updated_at
        }) {
            return Err("stored HumanTask lifecycle timestamps are invalid".into());
        }
        let claimant_is_consistent = match self.status {
            HumanTaskStatus::PendingActivation | HumanTaskStatus::Ready => {
                self.claimed_by.is_none() && self.claimed_at.is_none()
            }
            HumanTaskStatus::Claimed | HumanTaskStatus::Completed => {
                self.claimed_by.is_some() && self.claimed_at.is_some()
            }
            HumanTaskStatus::Expired | HumanTaskStatus::Cancelled => {
                self.claimed_by.is_some() == self.claimed_at.is_some()
            }
        };
        let terminal_is_consistent = if self.status.is_terminal() {
            self.decision_id.is_some() && self.terminal_at.is_some()
        } else {
            self.decision_id.is_none() && self.terminal_at.is_none()
        };
        if !claimant_is_consistent || !terminal_is_consistent {
            return Err("stored HumanTask lifecycle state is inconsistent".into());
        }
        Ok(())
    }

    fn apply_terminal_decision(
        &mut self,
        expected_version: u64,
        decision: &WorkflowDecision,
        status: HumanTaskStatus,
    ) -> Result<(), String> {
        if self.status.is_terminal() {
            return Err("terminal HumanTask cannot accept another decision".into());
        }
        decision.validate_for_task(self)?;
        let mut next = self.next_version(expected_version, decision.decided_at)?;
        if status == HumanTaskStatus::Completed {
            next.ensure_not_expired(next.updated_at)?;
        }
        next.status = status;
        next.decision_id = Some(decision.id);
        next.terminal_at = Some(next.updated_at);
        next.validate()?;
        *self = next;
        Ok(())
    }

    fn next_version(
        &self,
        expected_version: u64,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if expected_version == 0 || self.aggregate_version != expected_version {
            return Err("HumanTask aggregate version does not match the expected version".into());
        }
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("HumanTask transition time regressed".into());
        }
        let mut next = self.clone();
        next.aggregate_version = next
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "HumanTask aggregate version is exhausted".to_owned())?;
        next.updated_at = occurred_at;
        Ok(next)
    }

    fn ensure_not_expired(&self, occurred_at: DateTime<Utc>) -> Result<(), String> {
        if self
            .expires_at
            .is_some_and(|expires_at| occurred_at >= expires_at)
        {
            Err("HumanTask has expired".into())
        } else {
            Ok(())
        }
    }
}

fn valid_external_identity(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_EXTERNAL_IDENTITY_BYTES
        && !value.contains(['\0', '\r', '\n'])
}
