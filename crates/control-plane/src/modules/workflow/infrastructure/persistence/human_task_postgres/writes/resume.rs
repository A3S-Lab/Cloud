use super::super::rows::canonical_record;
use super::super::schema::{WorkflowResumeOutbox, WorkflowResumeReceipts};
use crate::infrastructure::{execute, fetch_optional, require_one_row, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, WorkflowDecisionId};
use crate::modules::workflow::domain::{FlowResumeReceipt, HumanTaskDecisionRecord};
use a3s_orm::{insert_into, select_from, update_table};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const RESUME_RECEIPT_MAX_BYTES: usize = 64 * 1024;

pub(in super::super) async fn lock_resume_outbox(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
) -> Result<(), PostgresPersistenceError> {
    fetch_optional::<i32, _>(
        transaction,
        select_from::<WorkflowResumeOutbox>()
            .select(WorkflowResumeOutbox::attempt_count())
            .filter(WorkflowResumeOutbox::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkflowResumeOutbox::workflow_decision_id().eq(workflow_decision_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)
    .map(|_| ())
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn retry_resume_delivery(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
    owner: Uuid,
    error: &str,
    failed_at: DateTime<Utc>,
    retry_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<WorkflowResumeOutbox>()
            .set(WorkflowResumeOutbox::state(), "pending")
            .set(WorkflowResumeOutbox::available_at(), retry_at)
            .set(WorkflowResumeOutbox::lease_owner(), None::<Uuid>)
            .set(
                WorkflowResumeOutbox::lease_expires_at(),
                None::<DateTime<Utc>>,
            )
            .set(WorkflowResumeOutbox::last_error(), error.to_owned())
            .set(WorkflowResumeOutbox::updated_at(), failed_at)
            .filter(WorkflowResumeOutbox::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkflowResumeOutbox::workflow_decision_id().eq(workflow_decision_id.as_uuid()))
            .filter(WorkflowResumeOutbox::state().eq("delivering"))
            .filter(WorkflowResumeOutbox::lease_owner().eq(owner))
            .filter(WorkflowResumeOutbox::updated_at().lte(failed_at))
            .filter(WorkflowResumeOutbox::lease_expires_at().gt(failed_at)),
    )
    .await?;
    require_resume_lease("schedule retry for", rows)
}

pub(in super::super) async fn conflict_resume_delivery(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
    owner: Uuid,
    error: &str,
    conflicted_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<WorkflowResumeOutbox>()
            .set(WorkflowResumeOutbox::state(), "conflicted")
            .set(WorkflowResumeOutbox::lease_owner(), None::<Uuid>)
            .set(
                WorkflowResumeOutbox::lease_expires_at(),
                None::<DateTime<Utc>>,
            )
            .set(WorkflowResumeOutbox::last_error(), error.to_owned())
            .set(WorkflowResumeOutbox::updated_at(), conflicted_at)
            .filter(WorkflowResumeOutbox::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkflowResumeOutbox::workflow_decision_id().eq(workflow_decision_id.as_uuid()))
            .filter(WorkflowResumeOutbox::state().eq("delivering"))
            .filter(WorkflowResumeOutbox::lease_owner().eq(owner))
            .filter(WorkflowResumeOutbox::updated_at().lte(conflicted_at))
            .filter(WorkflowResumeOutbox::lease_expires_at().gt(conflicted_at)),
    )
    .await?;
    require_resume_lease("mark conflict for", rows)
}

pub(in super::super) async fn insert_resume_receipt(
    transaction: &a3s_orm::PostgresTransaction,
    record: &HumanTaskDecisionRecord,
    receipt: &FlowResumeReceipt,
    recorded_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let receipt_json = canonical_record(receipt, RESUME_RECEIPT_MAX_BYTES, "Flow resume receipt")?;
    require_one_row(
        "Workflow resume receipt",
        execute(
            transaction,
            insert_into::<WorkflowResumeReceipts>()
                .value(
                    WorkflowResumeReceipts::organization_id(),
                    record.decision.organization_id.as_uuid(),
                )
                .value(
                    WorkflowResumeReceipts::project_id(),
                    record.decision.project_id.as_uuid(),
                )
                .value(
                    WorkflowResumeReceipts::workflow_decision_id(),
                    record.decision.id.as_uuid(),
                )
                .value(
                    WorkflowResumeReceipts::workflow_run_id(),
                    record.decision.workflow_run_id.as_uuid(),
                )
                .value(
                    WorkflowResumeReceipts::human_task_id(),
                    record.decision.human_task_id.as_uuid(),
                )
                .value(WorkflowResumeReceipts::flow_run_id(), receipt.flow_run_id())
                .value(
                    WorkflowResumeReceipts::flow_hook_id(),
                    receipt.flow_hook_id(),
                )
                .value(
                    WorkflowResumeReceipts::payload_digest(),
                    receipt.payload_digest().as_str(),
                )
                .value(
                    WorkflowResumeReceipts::disposition(),
                    receipt.disposition().as_str(),
                )
                .value(
                    WorkflowResumeReceipts::flow_event_sequence(),
                    receipt.flow_event_sequence(),
                )
                .value(
                    WorkflowResumeReceipts::flow_event_id(),
                    receipt.flow_event_id(),
                )
                .value(
                    WorkflowResumeReceipts::flow_event_at(),
                    receipt.flow_event_at(),
                )
                .value(WorkflowResumeReceipts::receipt_json(), receipt_json)
                .value(WorkflowResumeReceipts::recorded_at(), recorded_at),
        )
        .await?,
    )
}

pub(in super::super) async fn mark_resume_delivered(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_decision_id: WorkflowDecisionId,
    owner: Uuid,
    delivered_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<WorkflowResumeOutbox>()
            .set(WorkflowResumeOutbox::state(), "delivered")
            .set(WorkflowResumeOutbox::lease_owner(), None::<Uuid>)
            .set(
                WorkflowResumeOutbox::lease_expires_at(),
                None::<DateTime<Utc>>,
            )
            .set(WorkflowResumeOutbox::last_error(), None::<String>)
            .set(WorkflowResumeOutbox::updated_at(), delivered_at)
            .set(WorkflowResumeOutbox::delivered_at(), delivered_at)
            .filter(WorkflowResumeOutbox::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkflowResumeOutbox::workflow_decision_id().eq(workflow_decision_id.as_uuid()))
            .filter(WorkflowResumeOutbox::state().eq("delivering"))
            .filter(WorkflowResumeOutbox::lease_owner().eq(owner))
            .filter(WorkflowResumeOutbox::updated_at().lte(delivered_at))
            .filter(WorkflowResumeOutbox::lease_expires_at().gt(delivered_at)),
    )
    .await?;
    require_resume_lease("complete", rows)
}

fn require_resume_lease(action: &str, rows_affected: u64) -> Result<(), PostgresPersistenceError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "cannot {action} Workflow resume delivery because its lease is no longer owned"
        ))
        .into())
    }
}
