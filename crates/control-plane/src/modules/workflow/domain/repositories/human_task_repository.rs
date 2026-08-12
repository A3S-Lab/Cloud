use crate::modules::forms::domain::FormSubmission;
use crate::modules::shared_kernel::domain::{
    HumanTaskId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, WorkflowDecisionId,
};
use crate::modules::workflow::domain::{
    FlowResumePayload, FlowResumeReceipt, HumanTaskRecord, HumanTaskStatus, WorkflowDecision,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HumanTaskDecisionRecord {
    pub task: HumanTaskRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission: Option<FormSubmission>,
    pub decision: WorkflowDecision,
    pub resume_payload: FlowResumePayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_receipt: Option<FlowResumeReceipt>,
}

impl HumanTaskDecisionRecord {
    pub fn validate(&self) -> Result<(), String> {
        self.task.validate()?;
        self.decision.validate()?;
        self.resume_payload.validate()?;
        if !self.task.task.status.is_terminal()
            || self.task.task.decision_id != Some(self.decision.id)
            || self.decision.organization_id != self.task.task.organization_id
            || self.decision.project_id != self.task.task.project_id
            || self.decision.workflow_run_id != self.task.task.workflow_run_id
            || self.decision.human_task_id != self.task.task.id
            || self.decision.flow_run_id != self.task.task.flow_run_id
            || self.decision.flow_hook_id != self.task.task.flow_hook_id
            || self.decision.step_id != self.task.task.step_id
            || self.decision.step_attempt != self.task.task.step_attempt
            || self.decision.form_release != self.task.task.form_release
            || self.decision.assignment_policy != self.task.task.assignment_policy
            || self.decision.task_version.checked_add(1) != Some(self.task.task.aggregate_version)
            || self.decision.decided_at != self.task.task.updated_at
            || self.task.task.terminal_at != Some(self.decision.decided_at)
            || !matches!(
                (self.decision.outcome, self.task.task.status),
                (
                    crate::modules::workflow::domain::WorkflowDecisionOutcome::Submit
                        | crate::modules::workflow::domain::WorkflowDecisionOutcome::Approve
                        | crate::modules::workflow::domain::WorkflowDecisionOutcome::Reject,
                    HumanTaskStatus::Completed
                ) | (
                    crate::modules::workflow::domain::WorkflowDecisionOutcome::Expire,
                    HumanTaskStatus::Expired
                ) | (
                    crate::modules::workflow::domain::WorkflowDecisionOutcome::Cancel,
                    HumanTaskStatus::Cancelled
                )
            )
            || self.resume_payload != FlowResumePayload::from_decision(&self.decision)?
        {
            return Err("HumanTask decision record authority bindings are inconsistent".into());
        }
        match (&self.submission, self.decision.outcome.is_interactive()) {
            (Some(submission), true) => {
                submission.validate()?;
                if self.decision.form_submission_id != Some(submission.id)
                    || self.decision.form_submission_digest.as_ref() != Some(&submission.digest)
                    || submission.organization_id != self.decision.organization_id
                    || submission.project_id != self.decision.project_id
                    || submission.workflow_run_id != self.decision.workflow_run_id
                    || submission.human_task_id != self.decision.human_task_id
                    || submission.flow_run_id != self.decision.flow_run_id
                    || submission.flow_hook_id != self.decision.flow_hook_id
                    || submission.step_id != self.decision.step_id
                    || submission.step_attempt != self.decision.step_attempt
                    || submission.task_version != self.decision.task_version
                    || submission.form_release != self.decision.form_release
                    || submission.assignment_policy_id != self.decision.assignment_policy.id
                    || submission.assignment_policy_revision
                        != self.decision.assignment_policy.revision
                    || submission.assignment_policy_digest != self.decision.assignment_policy.digest
                    || submission.principal_id != self.decision.decided_by
                    || submission.accepted_at > self.decision.decided_at
                {
                    return Err("HumanTask decision submission binding is inconsistent".into());
                }
            }
            (None, false) => {}
            _ => {
                return Err(
                    "HumanTask decision submission presence does not match its outcome".into(),
                )
            }
        }
        if let Some(receipt) = &self.resume_receipt {
            receipt.validate()?;
            if receipt.flow_run_id != self.resume_payload.flow_run_id
                || receipt.flow_hook_id != self.resume_payload.flow_hook_id
                || receipt.workflow_decision_id != self.decision.id
                || receipt.payload_digest != self.resume_payload.digest
            {
                return Err("HumanTask Flow resume receipt binding is inconsistent".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HumanTaskResumeDelivery {
    pub record: HumanTaskDecisionRecord,
    pub attempt_count: u32,
    pub lease_owner: Uuid,
    pub claimed_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

impl HumanTaskResumeDelivery {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.record.resume_receipt.is_some()
            || self.attempt_count == 0
            || self.lease_owner.is_nil()
            || self.claimed_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.claimed_at)
            || self.lease_expires_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.lease_expires_at)
            || self.claimed_at < self.record.decision.decided_at
            || self.lease_expires_at <= self.claimed_at
        {
            return Err("Workflow resume delivery lease is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CreateHumanTaskWrite {
    pub record: HumanTaskRecord,
    pub hook_event_digest: Sha256Digest,
    pub hook_observed_at: DateTime<Utc>,
    pub event: DomainEventEnvelope,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ChangeHumanTaskWrite {
    pub record: HumanTaskRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct DecideHumanTaskWrite {
    pub record: HumanTaskDecisionRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HumanTaskWriteReference {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HumanTaskDecisionWriteReference {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
    pub workflow_decision_id: WorkflowDecisionId,
}

#[async_trait]
pub trait IHumanTaskRepository: Send + Sync {
    async fn create_from_hook(
        &self,
        write: CreateHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskRecord>, RepositoryError>;

    async fn find_task(
        &self,
        organization_id: OrganizationId,
        human_task_id: HumanTaskId,
    ) -> Result<Option<HumanTaskRecord>, RepositoryError>;

    async fn list_tasks(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        status: Option<HumanTaskStatus>,
        limit: usize,
    ) -> Result<Vec<HumanTaskRecord>, RepositoryError>;

    async fn replay_change(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<HumanTaskRecord>, RepositoryError>;

    async fn change_task(
        &self,
        write: ChangeHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskRecord>, RepositoryError>;

    async fn decide_task(
        &self,
        write: DecideHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskDecisionRecord>, RepositoryError>;

    async fn find_decision(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
    ) -> Result<Option<HumanTaskDecisionRecord>, RepositoryError>;

    async fn claim_resume_deliveries(
        &self,
        owner: Uuid,
        limit: usize,
        claimed_at: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Vec<HumanTaskResumeDelivery>, RepositoryError>;

    async fn retry_resume_delivery(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        error: &str,
        failed_at: DateTime<Utc>,
        retry_after: Duration,
    ) -> Result<(), RepositoryError>;

    async fn conflict_resume_delivery(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        error: &str,
        conflicted_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError>;

    async fn record_resume_receipt(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        receipt: FlowResumeReceipt,
        recorded_at: DateTime<Utc>,
    ) -> Result<HumanTaskDecisionRecord, RepositoryError>;
}
