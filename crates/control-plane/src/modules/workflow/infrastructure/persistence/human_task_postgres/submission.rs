use super::schema::FormSubmissions;
use crate::infrastructure::{execute, fetch_optional, require_one_row, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, FormSubmissionId, OrganizationId,
};
use crate::modules::workflow::domain::HumanTaskSubmission;
use a3s_form_core::FormInteractionOutcome;
use a3s_orm::expression::Selection;
use a3s_orm::{insert_into, select_from, DecodeError, Expression, FromRow, FromValue, Row};
use uuid::Uuid;

const HUMAN_TASK_SUBMISSION_RECORD_MAX_BYTES: usize = 2 * 1024 * 1024;

pub(super) async fn insert_human_task_submission(
    transaction: &a3s_orm::PostgresTransaction,
    submission: &HumanTaskSubmission,
) -> Result<(), PostgresPersistenceError> {
    submission.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!("HumanTaskSubmission is invalid: {error}"))
    })?;
    let form_id = parse_form_uuid(&submission.form_release.form_id, "form")?;
    let form_release_id = parse_form_uuid(&submission.form_release.release_id, "release")?;
    let record_json = String::from_utf8(
        canonical_json_bounded(
            submission,
            HUMAN_TASK_SUBMISSION_RECORD_MAX_BYTES,
            "HumanTaskSubmission record",
        )
        .map_err(PostgresPersistenceError::Invariant)?,
    )
    .map_err(|_| {
        PostgresPersistenceError::Invariant("HumanTaskSubmission JSON is not UTF-8".into())
    })?;
    require_one_row(
        "HumanTaskSubmission",
        execute(
            transaction,
            insert_into::<FormSubmissions>()
                .value(
                    FormSubmissions::organization_id(),
                    submission.organization_id.as_uuid(),
                )
                .value(
                    FormSubmissions::project_id(),
                    submission.project_id.as_uuid(),
                )
                .value(FormSubmissions::id(), submission.id.as_uuid())
                .value(
                    FormSubmissions::workflow_run_id(),
                    submission.workflow_run_id.as_uuid(),
                )
                .value(
                    FormSubmissions::human_task_id(),
                    submission.human_task_id.as_uuid(),
                )
                .value(FormSubmissions::form_id(), form_id)
                .value(FormSubmissions::form_release_id(), form_release_id)
                .value(
                    FormSubmissions::flow_run_id(),
                    submission.flow_run_id.as_str(),
                )
                .value(
                    FormSubmissions::flow_hook_id(),
                    submission.flow_hook_id.as_str(),
                )
                .value(FormSubmissions::step_id(), submission.step_id.as_str())
                .value(FormSubmissions::step_attempt(), submission.step_attempt)
                .value(
                    FormSubmissions::principal_id(),
                    submission.principal_id.as_uuid(),
                )
                .value(
                    FormSubmissions::authorization_decision_id(),
                    submission.authorization_decision.id.as_str(),
                )
                .value(
                    FormSubmissions::authorization_decision_digest(),
                    submission.authorization_decision.digest.as_str(),
                )
                .value(FormSubmissions::outcome(), outcome_name(submission.outcome))
                .value(
                    FormSubmissions::interaction_request_digest(),
                    submission.request_digest.as_str(),
                )
                .value(
                    FormSubmissions::interaction_submission_id(),
                    submission.interaction_submission_id.as_str(),
                )
                .value(
                    FormSubmissions::idempotency_key(),
                    submission.idempotency_key.as_str(),
                )
                .value(
                    FormSubmissions::candidate_value_digest(),
                    submission.candidate_value_digest.as_str(),
                )
                .value(
                    FormSubmissions::output_digest(),
                    submission.output_digest.as_str(),
                )
                .value(FormSubmissions::digest(), submission.digest.as_str())
                .value(
                    FormSubmissions::aggregate_version(),
                    submission.aggregate_version,
                )
                .value(FormSubmissions::record_json(), record_json)
                .value(FormSubmissions::submitted_at(), submission.submitted_at)
                .value(FormSubmissions::accepted_at(), submission.accepted_at),
        )
        .await?,
    )
}

pub(super) async fn load_human_task_submission(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    submission_id: FormSubmissionId,
) -> Result<Option<HumanTaskSubmission>, PostgresPersistenceError> {
    fetch_optional::<HumanTaskSubmissionRow, _>(
        transaction,
        submission_select()
            .filter(FormSubmissions::organization_id().eq(organization_id.as_uuid()))
            .filter(FormSubmissions::id().eq(submission_id.as_uuid())),
    )
    .await?
    .map(decode_submission)
    .transpose()
}

fn submission_select() -> a3s_orm::query::SelectQuery<FormSubmissions, HumanTaskSubmissionRow> {
    select_from::<FormSubmissions>().select(HumanTaskSubmissionSelection)
}

struct HumanTaskSubmissionSelection;

impl Selection for HumanTaskSubmissionSelection {
    type Output = HumanTaskSubmissionRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            FormSubmissions::organization_id().expression(),
            FormSubmissions::id().expression(),
            FormSubmissions::human_task_id().expression(),
            FormSubmissions::record_json().expression(),
        ]
    }
}

fn decode_submission(
    row: HumanTaskSubmissionRow,
) -> Result<HumanTaskSubmission, PostgresPersistenceError> {
    let submission: HumanTaskSubmission = serde_json::from_str(&row.record_json)?;
    submission.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored HumanTaskSubmission record is invalid: {error}"
        ))
    })?;
    if submission.organization_id.as_uuid() != row.organization_id
        || submission.id.as_uuid() != row.id
        || submission.human_task_id.as_uuid() != row.human_task_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored HumanTaskSubmission indexed authority drifted from its record".into(),
        ));
    }
    let canonical = String::from_utf8(
        canonical_json_bounded(
            &submission,
            HUMAN_TASK_SUBMISSION_RECORD_MAX_BYTES,
            "stored HumanTaskSubmission record",
        )
        .map_err(PostgresPersistenceError::Invariant)?,
    )
    .map_err(|_| {
        PostgresPersistenceError::Invariant("stored HumanTaskSubmission JSON is not UTF-8".into())
    })?;
    if canonical != row.record_json {
        return Err(PostgresPersistenceError::Invariant(
            "stored HumanTaskSubmission record is not canonical".into(),
        ));
    }
    Ok(submission)
}

fn parse_form_uuid(value: &str, label: &str) -> Result<Uuid, PostgresPersistenceError> {
    Uuid::parse_str(value).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "HumanTaskSubmission {label} identity is not a Cloud UUID: {error}"
        ))
    })
}

const fn outcome_name(outcome: FormInteractionOutcome) -> &'static str {
    match outcome {
        FormInteractionOutcome::Submit => "submit",
        FormInteractionOutcome::Approve => "approve",
        FormInteractionOutcome::Reject => "reject",
    }
}

struct HumanTaskSubmissionRow {
    organization_id: Uuid,
    id: Uuid,
    human_task_id: Uuid,
    record_json: String,
}

impl FromRow for HumanTaskSubmissionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            human_task_id: decode(row, 2)?,
            record_json: decode(row, 3)?,
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
