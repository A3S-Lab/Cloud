mod resume;

pub(super) use resume::{
    conflict_resume_delivery, insert_resume_receipt, lock_resume_outbox, mark_resume_delivered,
    retry_resume_delivery,
};

use super::rows::{canonical_record, form_uuid};
use super::schema::{HumanTasks, WorkflowDecisions, WorkflowHumanTaskInbox, WorkflowResumeOutbox};
use super::HUMAN_TASK_RECORD_MAX_BYTES;
use crate::infrastructure::{
    execute, require_one_row, store_audit, AuditWrite, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workflow::domain::{
    CreateHumanTaskWrite, HumanTaskDecisionRecord, HumanTaskRecord, HumanTaskStatus,
    WorkflowDecision,
};
use a3s_orm::{insert_into, update_table};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn insert_task(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskRecord,
) -> Result<bool, PostgresPersistenceError> {
    let task = &record.task;
    let form_id = form_uuid(&task.form_release.form_id, "form")?;
    let form_release_id = form_uuid(&task.form_release.release_id, "release")?;
    let task_json = canonical_record(task, HUMAN_TASK_RECORD_MAX_BYTES, "HumanTask")?;
    let interaction_json = canonical_record(
        &record.interaction,
        HUMAN_TASK_RECORD_MAX_BYTES,
        "HumanTask interaction spec",
    )?;
    let rows = execute(
        transaction,
        insert_into::<HumanTasks>()
            .value(
                HumanTasks::organization_id(),
                task.organization_id.as_uuid(),
            )
            .value(HumanTasks::project_id(), task.project_id.as_uuid())
            .value(HumanTasks::id(), task.id.as_uuid())
            .value(
                HumanTasks::workflow_run_id(),
                task.workflow_run_id.as_uuid(),
            )
            .value(HumanTasks::step_id(), task.step_id.as_str())
            .value(HumanTasks::step_attempt(), task.step_attempt)
            .value(HumanTasks::form_id(), form_id)
            .value(HumanTasks::form_release_id(), form_release_id)
            .value(
                HumanTasks::assignment_policy_id(),
                task.assignment_policy.id.as_str(),
            )
            .value(
                HumanTasks::assignment_policy_revision(),
                task.assignment_policy.revision,
            )
            .value(
                HumanTasks::assignment_policy_digest(),
                task.assignment_policy.digest.as_str(),
            )
            .value(HumanTasks::flow_run_id(), task.flow_run_id.as_str())
            .value(HumanTasks::flow_hook_id(), task.flow_hook_id.as_str())
            .value(HumanTasks::status(), task.status.as_str())
            .value(
                HumanTasks::claimed_by(),
                task.claimed_by.map(|value| value.as_uuid()),
            )
            .value(
                HumanTasks::decision_id(),
                task.decision_id.map(|value| value.as_uuid()),
            )
            .value(HumanTasks::aggregate_version(), task.aggregate_version)
            .value(HumanTasks::task_json(), task_json)
            .value(HumanTasks::interaction_spec_json(), interaction_json)
            .value(HumanTasks::interaction_request_json(), None::<String>)
            .value(HumanTasks::interaction_request_digest(), None::<String>)
            .value(
                HumanTasks::hook_event_sequence(),
                record.hook_event_sequence,
            )
            .value(HumanTasks::hook_event_id(), record.hook_event_id)
            .value(HumanTasks::created_at(), task.created_at)
            .value(HumanTasks::updated_at(), task.updated_at)
            .value(HumanTasks::due_at(), task.due_at)
            .value(HumanTasks::expires_at(), task.expires_at)
            .value(HumanTasks::claimed_at(), task.claimed_at)
            .value(HumanTasks::terminal_at(), task.terminal_at)
            .on_conflict((
                HumanTasks::organization_id(),
                HumanTasks::workflow_run_id(),
                HumanTasks::step_id(),
                HumanTasks::step_attempt(),
            ))
            .do_nothing(),
    )
    .await?;
    match rows {
        1 => Ok(true),
        0 => Ok(false),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "inserting HumanTask affected {rows} rows"
        ))),
    }
}

pub(super) async fn persist_task(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskRecord,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let task = &record.task;
    let task_json = canonical_record(task, HUMAN_TASK_RECORD_MAX_BYTES, "HumanTask")?;
    let request_json = record
        .interaction_request
        .as_ref()
        .map(|request| {
            canonical_record(
                request,
                HUMAN_TASK_RECORD_MAX_BYTES,
                "HumanTask Form interaction request",
            )
        })
        .transpose()?;
    let request_digest = record
        .interaction_request
        .as_ref()
        .map(|request| request.digest.as_str().to_owned());
    let rows = execute(
        transaction,
        update_table::<HumanTasks>()
            .set(HumanTasks::status(), task.status.as_str())
            .set(
                HumanTasks::claimed_by(),
                task.claimed_by.map(|value| value.as_uuid()),
            )
            .set(
                HumanTasks::decision_id(),
                task.decision_id.map(|value| value.as_uuid()),
            )
            .set(HumanTasks::aggregate_version(), task.aggregate_version)
            .set(HumanTasks::task_json(), task_json)
            .set(HumanTasks::interaction_request_json(), request_json)
            .set(HumanTasks::interaction_request_digest(), request_digest)
            .set(HumanTasks::updated_at(), task.updated_at)
            .set(HumanTasks::claimed_at(), task.claimed_at)
            .set(HumanTasks::terminal_at(), task.terminal_at)
            .filter(HumanTasks::organization_id().eq(task.organization_id.as_uuid()))
            .filter(HumanTasks::id().eq(task.id.as_uuid()))
            .filter(HumanTasks::aggregate_version().eq(expected_version)),
    )
    .await?;
    match rows {
        1 => Ok(()),
        0 => Err(RepositoryError::Conflict("HumanTask changed concurrently".into()).into()),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "updating HumanTask affected {rows} rows"
        ))),
    }
}

pub(super) async fn insert_hook_inbox(
    transaction: &a3s_orm::PostgresTransaction,
    write: &CreateHumanTaskWrite,
) -> Result<bool, PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<WorkflowHumanTaskInbox>()
            .value(
                WorkflowHumanTaskInbox::organization_id(),
                write.record.task.organization_id.as_uuid(),
            )
            .value(
                WorkflowHumanTaskInbox::workflow_run_id(),
                write.record.task.workflow_run_id.as_uuid(),
            )
            .value(
                WorkflowHumanTaskInbox::flow_sequence(),
                write.record.hook_event_sequence,
            )
            .value(
                WorkflowHumanTaskInbox::event_id(),
                write.record.hook_event_id,
            )
            .value(WorkflowHumanTaskInbox::event_key(), "flow.hook.created")
            .value(
                WorkflowHumanTaskInbox::event_digest(),
                write.hook_event_digest.as_str(),
            )
            .value(
                WorkflowHumanTaskInbox::observed_at(),
                write.hook_observed_at,
            )
            .value(
                WorkflowHumanTaskInbox::processed_at(),
                write.record.task.created_at,
            )
            .on_conflict((
                WorkflowHumanTaskInbox::organization_id(),
                WorkflowHumanTaskInbox::workflow_run_id(),
                WorkflowHumanTaskInbox::flow_sequence(),
            ))
            .do_nothing(),
    )
    .await?;
    match rows {
        1 => Ok(true),
        0 => Ok(false),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "inserting HumanTask Flow inbox event affected {rows} rows"
        ))),
    }
}

pub(super) async fn insert_decision(
    transaction: &a3s_orm::PostgresTransaction,
    decision: &WorkflowDecision,
) -> Result<(), PostgresPersistenceError> {
    let form_id = form_uuid(&decision.form_release.form_id, "form")?;
    let form_release_id = form_uuid(&decision.form_release.release_id, "release")?;
    let record_json = canonical_record(decision, HUMAN_TASK_RECORD_MAX_BYTES, "WorkflowDecision")?;
    require_one_row(
        "WorkflowDecision",
        execute(
            transaction,
            insert_into::<WorkflowDecisions>()
                .value(
                    WorkflowDecisions::organization_id(),
                    decision.organization_id.as_uuid(),
                )
                .value(
                    WorkflowDecisions::project_id(),
                    decision.project_id.as_uuid(),
                )
                .value(WorkflowDecisions::id(), decision.id.as_uuid())
                .value(
                    WorkflowDecisions::workflow_run_id(),
                    decision.workflow_run_id.as_uuid(),
                )
                .value(
                    WorkflowDecisions::human_task_id(),
                    decision.human_task_id.as_uuid(),
                )
                .value(
                    WorkflowDecisions::flow_run_id(),
                    decision.flow_run_id.as_str(),
                )
                .value(
                    WorkflowDecisions::flow_hook_id(),
                    decision.flow_hook_id.as_str(),
                )
                .value(WorkflowDecisions::step_id(), decision.step_id.as_str())
                .value(WorkflowDecisions::step_attempt(), decision.step_attempt)
                .value(WorkflowDecisions::task_version(), decision.task_version)
                .value(WorkflowDecisions::form_id(), form_id)
                .value(WorkflowDecisions::form_release_id(), form_release_id)
                .value(
                    WorkflowDecisions::assignment_policy_id(),
                    decision.assignment_policy.id.as_str(),
                )
                .value(
                    WorkflowDecisions::assignment_policy_revision(),
                    decision.assignment_policy.revision,
                )
                .value(
                    WorkflowDecisions::assignment_policy_digest(),
                    decision.assignment_policy.digest.as_str(),
                )
                .value(WorkflowDecisions::outcome(), decision.outcome.as_str())
                .value(
                    WorkflowDecisions::form_submission_id(),
                    decision.form_submission_id.map(|value| value.as_uuid()),
                )
                .value(
                    WorkflowDecisions::form_submission_digest(),
                    decision
                        .form_submission_digest
                        .as_ref()
                        .map(|value| value.as_str().to_owned()),
                )
                .value(
                    WorkflowDecisions::decided_by(),
                    decision.decided_by.as_uuid(),
                )
                .value(
                    WorkflowDecisions::authorization_decision_id(),
                    decision.authorization_decision.id.as_str(),
                )
                .value(
                    WorkflowDecisions::authorization_decision_digest(),
                    decision.authorization_decision.digest.as_str(),
                )
                .value(
                    WorkflowDecisions::output_digest(),
                    decision.output_digest.as_str(),
                )
                .value(WorkflowDecisions::digest(), decision.digest.as_str())
                .value(WorkflowDecisions::record_json(), record_json)
                .value(WorkflowDecisions::decided_at(), decision.decided_at),
        )
        .await?,
    )
}

pub(super) async fn insert_resume_outbox(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskDecisionRecord,
) -> Result<(), PostgresPersistenceError> {
    let payload_json = canonical_record(
        &record.resume_payload,
        HUMAN_TASK_RECORD_MAX_BYTES,
        "Flow resume payload",
    )?;
    let created_at = record.decision.decided_at;
    require_one_row(
        "Workflow resume Outbox entry",
        execute(
            transaction,
            insert_into::<WorkflowResumeOutbox>()
                .value(
                    WorkflowResumeOutbox::organization_id(),
                    record.decision.organization_id.as_uuid(),
                )
                .value(
                    WorkflowResumeOutbox::project_id(),
                    record.decision.project_id.as_uuid(),
                )
                .value(
                    WorkflowResumeOutbox::workflow_decision_id(),
                    record.decision.id.as_uuid(),
                )
                .value(
                    WorkflowResumeOutbox::workflow_run_id(),
                    record.decision.workflow_run_id.as_uuid(),
                )
                .value(
                    WorkflowResumeOutbox::human_task_id(),
                    record.decision.human_task_id.as_uuid(),
                )
                .value(
                    WorkflowResumeOutbox::flow_run_id(),
                    record.decision.flow_run_id.as_str(),
                )
                .value(
                    WorkflowResumeOutbox::flow_hook_id(),
                    record.decision.flow_hook_id.as_str(),
                )
                .value(WorkflowResumeOutbox::payload_json(), payload_json)
                .value(
                    WorkflowResumeOutbox::payload_digest(),
                    record.resume_payload.digest.as_str(),
                )
                .value(WorkflowResumeOutbox::state(), "pending")
                .value(WorkflowResumeOutbox::attempt_count(), 0)
                .value(WorkflowResumeOutbox::available_at(), created_at)
                .value(WorkflowResumeOutbox::lease_owner(), None::<Uuid>)
                .value(
                    WorkflowResumeOutbox::lease_expires_at(),
                    None::<DateTime<Utc>>,
                )
                .value(WorkflowResumeOutbox::last_error(), None::<String>)
                .value(WorkflowResumeOutbox::created_at(), created_at)
                .value(WorkflowResumeOutbox::updated_at(), created_at)
                .value(WorkflowResumeOutbox::delivered_at(), None::<DateTime<Utc>>),
        )
        .await?,
    )
}

pub(super) async fn store_task_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskRecord,
    actor_principal_id: Option<crate::modules::shared_kernel::domain::PrincipalId>,
    request_id: Uuid,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.task.organization_id.as_uuid(),
            actor_id: actor_principal_id.map(|value| value.as_uuid()),
            action,
            aggregate_id: record.task.id.as_uuid(),
            occurred_at: record.task.updated_at,
            request_id,
            attribution_scope: AuditWrite::project_attribution(record.task.project_id, None),
            details: serde_json::json!({
                "projectId": record.task.project_id,
                "workflowRunId": record.task.workflow_run_id,
                "stepId": record.task.step_id,
                "stepAttempt": record.task.step_attempt,
                "flowRunId": record.task.flow_run_id,
                "flowHookId": record.task.flow_hook_id,
                "status": record.task.status,
                "aggregateVersion": record.task.aggregate_version,
                "claimedBy": record.task.claimed_by,
                "decisionId": record.task.decision_id,
            }),
        },
    )
    .await
}

pub(super) const fn task_action(status: HumanTaskStatus) -> &'static str {
    match status {
        HumanTaskStatus::PendingActivation => "workflow.human-task.created",
        HumanTaskStatus::Ready => "workflow.human-task.ready",
        HumanTaskStatus::Claimed => "workflow.human-task.claimed",
        HumanTaskStatus::Completed => "workflow.human-task.completed",
        HumanTaskStatus::Expired => "workflow.human-task.expired",
        HumanTaskStatus::Cancelled => "workflow.human-task.cancelled",
    }
}
