use super::schema::{
    FormReleases, HumanTasks, WorkflowDecisions, WorkflowHumanTaskInbox, WorkflowResumeOutbox,
    WorkflowResumeReceipts, WorkflowRuns,
};
use super::HUMAN_TASK_RECORD_MAX_BYTES;
use crate::infrastructure::{fetch_all, fetch_optional, PostgresPersistenceError};
use crate::modules::forms::infrastructure::persistence::load_form_submission;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, FormSubmissionId, HumanTaskId, OrganizationId, RepositoryError,
    WorkflowDecisionId,
};
use crate::modules::workflow::domain::{
    CreateHumanTaskWrite, FlowResumePayload, HumanTask, HumanTaskDecisionRecord,
    HumanTaskInteractionSpec, HumanTaskRecord, WorkflowDecision,
};
use a3s_orm::expression::Selection;
use a3s_orm::{select_from, DecodeError, Expression, FromRow, FromValue, OrderDirection, Row};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub(super) fn task_select() -> a3s_orm::query::SelectQuery<HumanTasks, HumanTaskRow> {
    select_from::<HumanTasks>().select(HumanTaskSelection)
}

pub(super) async fn find_task_row(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    human_task_id: HumanTaskId,
    for_update: bool,
) -> Result<Option<HumanTaskRow>, PostgresPersistenceError> {
    let mut query = task_select()
        .filter(HumanTasks::organization_id().eq(organization_id.as_uuid()))
        .filter(HumanTasks::id().eq(human_task_id.as_uuid()));
    if for_update {
        query = query.for_update();
    }
    fetch_optional(transaction, query).await
}

pub(super) async fn load_task(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    human_task_id: HumanTaskId,
    for_update: bool,
) -> Result<HumanTaskRecord, PostgresPersistenceError> {
    find_task_row(transaction, organization_id, human_task_id, for_update)
        .await?
        .map(decode_task)
        .transpose()?
        .ok_or(RepositoryError::NotFound.into())
}

pub(super) async fn task_authority_exists(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskRecord,
) -> Result<bool, PostgresPersistenceError> {
    let task = &record.task;
    let form_id = form_uuid(&task.form_release.form_id, "form")?;
    let form_release_id = form_uuid(&task.form_release.release_id, "release")?;
    let workflow_run_exists = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<WorkflowRuns>()
            .select(WorkflowRuns::id())
            .filter(WorkflowRuns::organization_id().eq(task.organization_id.as_uuid()))
            .filter(WorkflowRuns::project_id().eq(task.project_id.as_uuid()))
            .filter(WorkflowRuns::id().eq(task.workflow_run_id.as_uuid())),
    )
    .await?
    .is_some();
    if !workflow_run_exists {
        return Ok(false);
    }
    Ok(fetch_optional::<Uuid, _>(
        transaction,
        select_from::<FormReleases>()
            .select(FormReleases::id())
            .filter(FormReleases::organization_id().eq(task.organization_id.as_uuid()))
            .filter(FormReleases::project_id().eq(task.project_id.as_uuid()))
            .filter(FormReleases::form_id().eq(form_id))
            .filter(FormReleases::id().eq(form_release_id)),
    )
    .await?
    .is_some())
}

pub(super) async fn find_task_identity_collisions(
    transaction: &a3s_orm::PostgresTransaction,
    requested: &HumanTaskRecord,
) -> Result<Vec<HumanTaskRecord>, PostgresPersistenceError> {
    let task = &requested.task;
    fetch_all::<HumanTaskRow, _>(
        transaction,
        task_select()
            .filter(HumanTasks::organization_id().eq(task.organization_id.as_uuid()))
            .filter(
                HumanTasks::id()
                    .eq(task.id.as_uuid())
                    .or(HumanTasks::workflow_run_id()
                        .eq(task.workflow_run_id.as_uuid())
                        .and(HumanTasks::step_id().eq(task.step_id.as_str()))
                        .and(HumanTasks::step_attempt().eq(task.step_attempt)))
                    .or(HumanTasks::flow_run_id()
                        .eq(task.flow_run_id.as_str())
                        .and(HumanTasks::flow_hook_id().eq(task.flow_hook_id.as_str())))
                    .or(HumanTasks::workflow_run_id()
                        .eq(task.workflow_run_id.as_uuid())
                        .and(HumanTasks::hook_event_id().eq(requested.hook_event_id))),
            )
            .order_by(HumanTasks::id(), OrderDirection::Asc),
    )
    .await?
    .into_iter()
    .map(decode_task)
    .collect()
}

pub(super) async fn hook_inbox_matches(
    transaction: &a3s_orm::PostgresTransaction,
    write: &CreateHumanTaskWrite,
) -> Result<bool, PostgresPersistenceError> {
    let matched_event = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<WorkflowHumanTaskInbox>()
            .select(WorkflowHumanTaskInbox::event_id())
            .filter(
                WorkflowHumanTaskInbox::organization_id().eq(write
                    .record
                    .task
                    .organization_id
                    .as_uuid()),
            )
            .filter(
                WorkflowHumanTaskInbox::workflow_run_id().eq(write
                    .record
                    .task
                    .workflow_run_id
                    .as_uuid()),
            )
            .filter(WorkflowHumanTaskInbox::flow_sequence().eq(write.record.hook_event_sequence))
            .filter(WorkflowHumanTaskInbox::event_id().eq(write.record.hook_event_id))
            .filter(WorkflowHumanTaskInbox::event_key().eq("flow.hook.created"))
            .filter(WorkflowHumanTaskInbox::event_digest().eq(write.hook_event_digest.as_str()))
            .filter(WorkflowHumanTaskInbox::observed_at().eq(write.hook_observed_at)),
    )
    .await?;
    Ok(matched_event.is_some())
}

pub(super) fn decode_task(row: HumanTaskRow) -> Result<HumanTaskRecord, PostgresPersistenceError> {
    let task: HumanTask = decode_canonical_record(&row.task_json, "HumanTask")?;
    let interaction: HumanTaskInteractionSpec =
        decode_canonical_record(&row.interaction_spec_json, "HumanTask interaction spec")?;
    let interaction_request = row
        .interaction_request_json
        .as_deref()
        .map(|value| decode_canonical_record(value, "HumanTask Form interaction request"))
        .transpose()?;
    let record = HumanTaskRecord {
        task,
        interaction,
        interaction_request,
        hook_event_sequence: row.hook_event_sequence,
        hook_event_id: row.hook_event_id,
    };
    record.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored HumanTask is invalid: {error}"))
    })?;
    if record.task.organization_id.as_uuid() != row.organization_id
        || record.task.project_id.as_uuid() != row.project_id
        || record.task.id.as_uuid() != row.id
        || record.task.workflow_run_id.as_uuid() != row.workflow_run_id
        || record.task.status.as_str() != row.status
        || record.task.aggregate_version != row.aggregate_version
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored HumanTask indexed state drifted from its record".into(),
        ));
    }
    Ok(record)
}

fn decision_select() -> a3s_orm::query::SelectQuery<WorkflowDecisions, HumanTaskDecisionRow> {
    select_from::<WorkflowDecisions>()
        .select(HumanTaskDecisionSelection)
        .inner_join::<WorkflowResumeOutbox>(
            WorkflowResumeOutbox::organization_id()
                .eq_column(WorkflowDecisions::organization_id())
                .and(
                    WorkflowResumeOutbox::workflow_decision_id().eq_column(WorkflowDecisions::id()),
                ),
        )
        .left_join::<WorkflowResumeReceipts>(
            WorkflowResumeReceipts::organization_id()
                .eq_column(WorkflowDecisions::organization_id())
                .and(
                    WorkflowResumeReceipts::workflow_decision_id()
                        .eq_column(WorkflowDecisions::id()),
                ),
        )
}

pub(super) async fn find_decision_row(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
) -> Result<Option<HumanTaskDecisionRow>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        decision_select()
            .filter(WorkflowDecisions::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkflowDecisions::id().eq(workflow_decision_id.as_uuid())),
    )
    .await
}

pub(super) async fn load_decision_record(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
) -> Result<HumanTaskDecisionRecord, PostgresPersistenceError> {
    let row = find_decision_row(transaction, organization_id, workflow_decision_id)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    decode_decision_record(transaction, row).await
}

pub(super) async fn decode_decision_record(
    transaction: &a3s_orm::PostgresTransaction,
    row: HumanTaskDecisionRow,
) -> Result<HumanTaskDecisionRecord, PostgresPersistenceError> {
    let decision: WorkflowDecision = decode_canonical_record(&row.record_json, "WorkflowDecision")?;
    if decision.organization_id.as_uuid() != row.organization_id
        || decision.id.as_uuid() != row.id
        || decision.human_task_id.as_uuid() != row.human_task_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored WorkflowDecision indexed authority drifted from its record".into(),
        ));
    }
    let task = load_task(
        transaction,
        OrganizationId::from_uuid(row.organization_id),
        HumanTaskId::from_uuid(row.human_task_id),
        false,
    )
    .await?;
    let submission = match row.form_submission_id {
        Some(id) => Some(
            load_form_submission(
                transaction,
                OrganizationId::from_uuid(row.organization_id),
                FormSubmissionId::from_uuid(id),
            )
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "WorkflowDecision is missing its FormSubmission".into(),
                )
            })?,
        ),
        None => None,
    };
    let resume_payload: FlowResumePayload =
        decode_canonical_record(&row.payload_json, "Flow resume payload")?;
    let resume_receipt = row
        .receipt_json
        .as_deref()
        .map(|value| decode_canonical_record(value, "Flow resume receipt"))
        .transpose()?;
    let record = HumanTaskDecisionRecord {
        task,
        submission,
        decision,
        resume_payload,
        resume_receipt,
    };
    record.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored HumanTask decision record is invalid: {error}"
        ))
    })?;
    Ok(record)
}

pub(super) fn canonical_record<T: Serialize>(
    value: &T,
    maximum_bytes: usize,
    label: &str,
) -> Result<String, PostgresPersistenceError> {
    let bytes = canonical_json_bounded(value, maximum_bytes, label)
        .map_err(PostgresPersistenceError::Invariant)?;
    String::from_utf8(bytes)
        .map_err(|_| PostgresPersistenceError::Invariant(format!("{label} JSON is not UTF-8")))
}

fn decode_canonical_record<T>(value: &str, label: &str) -> Result<T, PostgresPersistenceError>
where
    T: serde::de::DeserializeOwned + Serialize,
{
    let decoded: T = serde_json::from_str(value)?;
    if canonical_record(&decoded, HUMAN_TASK_RECORD_MAX_BYTES, label)? != value {
        return Err(PostgresPersistenceError::Invariant(format!(
            "stored {label} is not canonical"
        )));
    }
    Ok(decoded)
}

pub(super) fn form_uuid(value: &str, label: &str) -> Result<Uuid, PostgresPersistenceError> {
    Uuid::parse_str(value).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "HumanTask {label} identity is not a Cloud UUID: {error}"
        ))
    })
}

pub(super) struct HumanTaskRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    workflow_run_id: Uuid,
    status: String,
    aggregate_version: u64,
    task_json: String,
    interaction_spec_json: String,
    interaction_request_json: Option<String>,
    hook_event_sequence: u64,
    hook_event_id: Uuid,
}

struct HumanTaskSelection;

impl Selection for HumanTaskSelection {
    type Output = HumanTaskRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            HumanTasks::organization_id().expression(),
            HumanTasks::project_id().expression(),
            HumanTasks::id().expression(),
            HumanTasks::workflow_run_id().expression(),
            HumanTasks::status().expression(),
            HumanTasks::aggregate_version().expression(),
            HumanTasks::task_json().expression(),
            HumanTasks::interaction_spec_json().expression(),
            HumanTasks::interaction_request_json().expression(),
            HumanTasks::hook_event_sequence().expression(),
            HumanTasks::hook_event_id().expression(),
        ]
    }
}

impl FromRow for HumanTaskRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            workflow_run_id: decode(row, 3)?,
            status: decode(row, 4)?,
            aggregate_version: decode(row, 5)?,
            task_json: decode(row, 6)?,
            interaction_spec_json: decode(row, 7)?,
            interaction_request_json: decode(row, 8)?,
            hook_event_sequence: decode(row, 9)?,
            hook_event_id: decode(row, 10)?,
        })
    }
}

pub(super) struct HumanTaskDecisionRow {
    organization_id: Uuid,
    id: Uuid,
    human_task_id: Uuid,
    record_json: String,
    form_submission_id: Option<Uuid>,
    payload_json: String,
    receipt_json: Option<String>,
}

struct HumanTaskDecisionSelection;

impl Selection for HumanTaskDecisionSelection {
    type Output = HumanTaskDecisionRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkflowDecisions::organization_id().expression(),
            WorkflowDecisions::id().expression(),
            WorkflowDecisions::human_task_id().expression(),
            WorkflowDecisions::record_json().expression(),
            WorkflowDecisions::form_submission_id().expression(),
            WorkflowResumeOutbox::payload_json().expression(),
            WorkflowResumeReceipts::receipt_json().expression(),
        ]
    }
}

impl FromRow for HumanTaskDecisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            human_task_id: decode(row, 2)?,
            record_json: decode(row, 3)?,
            form_submission_id: decode(row, 4)?,
            payload_json: decode(row, 5)?,
            receipt_json: decode(row, 6)?,
        })
    }
}

pub(super) struct ResumeDeliveryClaimRow {
    pub(super) organization_id: Uuid,
    pub(super) workflow_decision_id: Uuid,
    pub(super) attempt_count: i32,
    pub(super) claimed_at: DateTime<Utc>,
    pub(super) lease_expires_at: DateTime<Utc>,
}

impl FromRow for ResumeDeliveryClaimRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            workflow_decision_id: decode(row, 1)?,
            attempt_count: decode(row, 2)?,
            claimed_at: decode(row, 3)?,
            lease_expires_at: decode(row, 4)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
