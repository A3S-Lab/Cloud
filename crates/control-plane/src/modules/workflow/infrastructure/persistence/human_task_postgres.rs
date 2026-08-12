mod rows;
mod schema;
mod writes;

use self::rows::{
    decode_decision_record, decode_task, find_decision_row, find_task_identity_collisions,
    find_task_row, hook_inbox_matches, load_decision_record, load_task, task_authority_exists,
    task_select, HumanTaskRow, ResumeDeliveryClaimRow, ResumeDeliveryClaimSelection,
};
use self::schema::{HumanTasks, WorkflowResumeCandidates, WorkflowResumeOutbox};
use self::writes::{
    conflict_resume_delivery, insert_decision, insert_hook_inbox, insert_resume_outbox,
    insert_resume_receipt, insert_task, lock_resume_outbox, mark_resume_delivered, persist_task,
    retry_resume_delivery, store_task_audit, task_action,
};
use crate::infrastructure::{
    fetch_all, idempotency_replay, is_foreign_key_violation, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::forms::infrastructure::persistence::insert_form_submission;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, HumanTaskId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, WorkflowDecisionId,
};
use crate::modules::workflow::domain::repositories::{
    HumanTaskDecisionWriteReference, HumanTaskWriteReference,
};
use crate::modules::workflow::domain::{
    ChangeHumanTaskWrite, CreateHumanTaskWrite, DecideHumanTaskWrite, FlowResumeReceipt,
    HumanTaskDecisionRecord, HumanTaskRecord, HumanTaskResumeDelivery, HumanTaskStatus,
    IHumanTaskRepository,
};
use a3s_orm::{select_from, update_table, OrderDirection, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;

const HUMAN_TASK_RECORD_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct PostgresHumanTaskRepository {
    executor: PostgresExecutor,
}

impl PostgresHumanTaskRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IHumanTaskRepository for PostgresHumanTaskRepository {
    async fn create_from_hook(
        &self,
        write: CreateHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if !task_authority_exists(transaction, &write.record).await? {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let collisions =
                        find_task_identity_collisions(transaction, &write.record).await?;
                    if !collisions.is_empty() {
                        let replay = replay_task_collisions(collisions, &write.record)?;
                        if !hook_inbox_matches(transaction, &write).await? {
                            return Err(RepositoryError::Conflict(
                                "Flow hook inbox evidence differs from the HumanTask replay".into(),
                            )
                            .into());
                        }
                        return Ok(replay);
                    }
                    let inserted = match insert_task(transaction, &write.record).await {
                        Ok(inserted) => inserted,
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    if !inserted {
                        let collisions =
                            find_task_identity_collisions(transaction, &write.record).await?;
                        let replay = replay_task_collisions(collisions, &write.record)?;
                        if !hook_inbox_matches(transaction, &write).await? {
                            return Err(RepositoryError::Conflict(
                                "Flow hook inbox evidence differs from the HumanTask replay".into(),
                            )
                            .into());
                        }
                        return Ok(replay);
                    }
                    if !insert_hook_inbox(transaction, &write).await? {
                        return Err(RepositoryError::Conflict(
                            "Flow hook event was already processed by another consumer".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_task_audit(
                        transaction,
                        &write.record,
                        None,
                        write.request_id,
                        "workflow.human-task.created",
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_task(
        &self,
        organization_id: OrganizationId,
        human_task_id: HumanTaskId,
    ) -> Result<Option<HumanTaskRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    find_task_row(transaction, organization_id, human_task_id, false)
                        .await?
                        .map(decode_task)
                        .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_tasks(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        status: Option<HumanTaskStatus>,
        limit: usize,
    ) -> Result<Vec<HumanTaskRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = u64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("HumanTask list limit exceeds the supported range".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut query = task_select()
                        .filter(HumanTasks::organization_id().eq(organization_id.as_uuid()))
                        .filter(HumanTasks::project_id().eq(project_id.as_uuid()));
                    if let Some(status) = status {
                        query = query.filter(HumanTasks::status().eq(status.as_str()));
                    }
                    query = query
                        .order_by(HumanTasks::created_at(), OrderDirection::Asc)
                        .order_by(HumanTasks::id(), OrderDirection::Asc)
                        .limit(limit);
                    fetch_all::<HumanTaskRow, _>(transaction, query)
                        .await?
                        .into_iter()
                        .map(decode_task)
                        .collect()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_change(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<HumanTaskRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) =
                        idempotency_replay::<HumanTaskWriteReference>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    load_task(
                        transaction,
                        replay.value.organization_id,
                        replay.value.human_task_id,
                        false,
                    )
                    .await
                    .map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn change_task(
        &self,
        write: ChangeHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<HumanTaskWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_task(
                                transaction,
                                replay.value.organization_id,
                                replay.value.human_task_id,
                                false,
                            )
                            .await?,
                            replayed: true,
                        });
                    }
                    let existing = load_task(
                        transaction,
                        write.record.task.organization_id,
                        write.record.task.id,
                        true,
                    )
                    .await?;
                    validate_task_transition(
                        &existing,
                        &write.record,
                        write.expected_version,
                        write.actor_principal_id,
                    )?;
                    persist_task(transaction, &write.record, write.expected_version).await?;
                    store_outbox(transaction, &write.event).await?;
                    store_task_audit(
                        transaction,
                        &write.record,
                        Some(write.actor_principal_id),
                        write.request_id,
                        task_action(write.record.task.status),
                    )
                    .await?;
                    let reference = HumanTaskWriteReference {
                        organization_id: write.record.task.organization_id,
                        human_task_id: write.record.task.id,
                    };
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_decision(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<HumanTaskDecisionRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) = idempotency_replay::<HumanTaskDecisionWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_replayed_decision(transaction, replay.value)
                        .await
                        .map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn decide_task(
        &self,
        write: DecideHumanTaskWrite,
    ) -> Result<IdempotentWrite<HumanTaskDecisionRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<HumanTaskDecisionWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_replayed_decision(transaction, replay.value).await?,
                            replayed: true,
                        });
                    }
                    let existing = load_task(
                        transaction,
                        write.record.task.task.organization_id,
                        write.record.task.task.id,
                        true,
                    )
                    .await?;
                    validate_decision_transition(
                        &existing,
                        &write.record,
                        write.expected_version,
                        write.actor_principal_id,
                    )?;
                    if let Some(submission) = &write.record.submission {
                        insert_form_submission(transaction, submission).await?;
                    }
                    insert_decision(transaction, &write.record.decision).await?;
                    persist_task(transaction, &write.record.task, write.expected_version).await?;
                    insert_resume_outbox(transaction, &write.record).await?;
                    store_outbox(transaction, &write.event).await?;
                    store_task_audit(
                        transaction,
                        &write.record.task,
                        Some(write.actor_principal_id),
                        write.request_id,
                        task_action(write.record.task.task.status),
                    )
                    .await?;
                    let reference = HumanTaskDecisionWriteReference {
                        organization_id: write.record.task.task.organization_id,
                        human_task_id: write.record.task.task.id,
                        workflow_decision_id: write.record.decision.id,
                    };
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_decision(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
    ) -> Result<Option<HumanTaskDecisionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    match find_decision_row(transaction, organization_id, workflow_decision_id)
                        .await?
                    {
                        Some(row) => decode_decision_record(transaction, row).await.map(Some),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn claim_resume_deliveries(
        &self,
        owner: Uuid,
        limit: usize,
        claimed_at: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Vec<HumanTaskResumeDelivery>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if owner.is_nil() || lease_duration.is_zero() {
            return Err(RepositoryError::Conflict(
                "Workflow resume delivery requires a valid owner and positive lease".into(),
            ));
        }
        let limit = u64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict(
                "Workflow resume delivery limit exceeds the supported range".into(),
            )
        })?;
        let claimed_at = canonical_timestamp(claimed_at);
        let lease_millis = i64::try_from(lease_duration.as_millis()).map_err(|_| {
            RepositoryError::Conflict(
                "Workflow resume delivery lease exceeds the supported duration".into(),
            )
        })?;
        let lease_expires_at = claimed_at
            .checked_add_signed(chrono::Duration::milliseconds(lease_millis))
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Workflow resume delivery lease expiry is outside the supported range".into(),
                )
            })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let candidates = select_from::<WorkflowResumeOutbox>()
                        .select((
                            WorkflowResumeOutbox::organization_id(),
                            WorkflowResumeOutbox::workflow_decision_id(),
                        ))
                        .filter(WorkflowResumeOutbox::available_at().lte(claimed_at))
                        .filter(
                            WorkflowResumeOutbox::state().eq("pending").or(
                                WorkflowResumeOutbox::state()
                                    .eq("delivering")
                                    .and(WorkflowResumeOutbox::lease_expires_at().lte(claimed_at)),
                            ),
                        )
                        .order_by(WorkflowResumeOutbox::available_at(), OrderDirection::Asc)
                        .order_by(WorkflowResumeOutbox::created_at(), OrderDirection::Asc)
                        .order_by(
                            WorkflowResumeOutbox::workflow_decision_id(),
                            OrderDirection::Asc,
                        )
                        .limit(limit)
                        .for_update_of::<WorkflowResumeOutbox>()
                        .skip_locked()
                        .as_cte::<WorkflowResumeCandidates>();
                    let rows = fetch_all::<ResumeDeliveryClaimRow, _>(
                        transaction,
                        update_table::<WorkflowResumeOutbox>()
                            .with(candidates)
                            .set(WorkflowResumeOutbox::state(), "delivering")
                            .set_expression(
                                WorkflowResumeOutbox::attempt_count(),
                                WorkflowResumeOutbox::attempt_count() + 1,
                            )
                            .set(WorkflowResumeOutbox::lease_owner(), owner)
                            .set(WorkflowResumeOutbox::lease_expires_at(), lease_expires_at)
                            .set(WorkflowResumeOutbox::last_error(), None::<String>)
                            .set(WorkflowResumeOutbox::updated_at(), claimed_at)
                            .from::<WorkflowResumeCandidates>()
                            .filter(
                                WorkflowResumeOutbox::organization_id()
                                    .eq_column(WorkflowResumeCandidates::organization_id()),
                            )
                            .filter(
                                WorkflowResumeOutbox::workflow_decision_id()
                                    .eq_column(WorkflowResumeCandidates::workflow_decision_id()),
                            )
                            .returning(ResumeDeliveryClaimSelection),
                    )
                    .await?;
                    let mut deliveries = Vec::with_capacity(rows.len());
                    for row in rows {
                        let attempt_count = u32::try_from(row.attempt_count).map_err(|_| {
                            PostgresPersistenceError::Invariant(
                                "Workflow resume delivery attempt count is invalid".into(),
                            )
                        })?;
                        let delivery = HumanTaskResumeDelivery {
                            record: load_decision_record(
                                transaction,
                                OrganizationId::from_uuid(row.organization_id),
                                WorkflowDecisionId::from_uuid(row.workflow_decision_id),
                            )
                            .await?,
                            attempt_count,
                            lease_owner: owner,
                            claimed_at: row.claimed_at,
                            lease_expires_at: row.lease_expires_at,
                        };
                        delivery.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "claimed Workflow resume delivery is invalid: {error}"
                            ))
                        })?;
                        deliveries.push(delivery);
                    }
                    Ok(deliveries)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn retry_resume_delivery(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        error: &str,
        failed_at: DateTime<Utc>,
        retry_after: Duration,
    ) -> Result<(), RepositoryError> {
        if owner.is_nil() || error.trim().is_empty() || retry_after.is_zero() {
            return Err(RepositoryError::Conflict(
                "Workflow resume delivery retry request is invalid".into(),
            ));
        }
        let retry_millis = i64::try_from(retry_after.as_millis()).map_err(|_| {
            RepositoryError::Conflict(
                "Workflow resume delivery retry exceeds the supported duration".into(),
            )
        })?;
        let failed_at = canonical_timestamp(failed_at);
        let retry_at = failed_at
            .checked_add_signed(chrono::Duration::milliseconds(retry_millis))
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Workflow resume delivery retry time is outside the supported range".into(),
                )
            })?;
        let error = bounded_delivery_error(error);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    retry_resume_delivery(
                        transaction,
                        organization_id,
                        workflow_decision_id,
                        owner,
                        &error,
                        failed_at,
                        retry_at,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn conflict_resume_delivery(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        error: &str,
        conflicted_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        if owner.is_nil() || error.trim().is_empty() {
            return Err(RepositoryError::Conflict(
                "Workflow resume delivery conflict request is invalid".into(),
            ));
        }
        let conflicted_at = canonical_timestamp(conflicted_at);
        let error = bounded_delivery_error(error);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    conflict_resume_delivery(
                        transaction,
                        organization_id,
                        workflow_decision_id,
                        owner,
                        &error,
                        conflicted_at,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn record_resume_receipt(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
        owner: Uuid,
        receipt: FlowResumeReceipt,
        recorded_at: DateTime<Utc>,
    ) -> Result<HumanTaskDecisionRecord, RepositoryError> {
        receipt.validate().map_err(RepositoryError::Storage)?;
        if owner.is_nil() {
            return Err(RepositoryError::Conflict(
                "Workflow resume receipt requires a valid delivery owner".into(),
            ));
        }
        let recorded_at = canonical_timestamp(recorded_at);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_resume_outbox(transaction, organization_id, workflow_decision_id).await?;
                    let mut record =
                        load_decision_record(transaction, organization_id, workflow_decision_id)
                            .await?;
                    if let Some(existing) = &record.resume_receipt {
                        if existing == &receipt {
                            return Ok(record);
                        }
                        return Err(RepositoryError::Conflict(
                            "Workflow resume receipt already records different Flow evidence"
                                .into(),
                        )
                        .into());
                    }
                    if receipt.workflow_decision_id != record.decision.id
                        || receipt.payload_digest != record.resume_payload.digest
                        || receipt.flow_run_id != record.resume_payload.flow_run_id
                        || receipt.flow_hook_id != record.resume_payload.flow_hook_id
                        || recorded_at < receipt.hook_received_at
                    {
                        return Err(RepositoryError::Conflict(
                            "Workflow resume receipt does not match the pending delivery".into(),
                        )
                        .into());
                    }
                    insert_resume_receipt(transaction, &record, &receipt, recorded_at).await?;
                    mark_resume_delivered(
                        transaction,
                        organization_id,
                        workflow_decision_id,
                        owner,
                        recorded_at,
                    )
                    .await?;
                    record.resume_receipt = Some(receipt);
                    record
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn load_replayed_decision(
    transaction: &a3s_orm::PostgresTransaction,
    reference: HumanTaskDecisionWriteReference,
) -> Result<HumanTaskDecisionRecord, PostgresPersistenceError> {
    let record = load_decision_record(
        transaction,
        reference.organization_id,
        reference.workflow_decision_id,
    )
    .await?;
    if record.task.task.id != reference.human_task_id {
        return Err(PostgresPersistenceError::Invariant(
            "HumanTask decision idempotency reference authority drifted".into(),
        ));
    }
    Ok(record)
}

fn bounded_delivery_error(error: &str) -> String {
    const MAX_BYTES: usize = 16 * 1024;
    let normalized = error.trim();
    if normalized.len() <= MAX_BYTES {
        return normalized.to_owned();
    }
    let mut end = MAX_BYTES;
    while !normalized.is_char_boundary(end) {
        end -= 1;
    }
    normalized[..end].to_owned()
}

fn validate_task_transition(
    existing: &HumanTaskRecord,
    next: &HumanTaskRecord,
    expected_version: u64,
    actor_principal_id: PrincipalId,
) -> Result<(), PostgresPersistenceError> {
    if existing.task.aggregate_version != expected_version
        || next.task.aggregate_version
            != expected_version.checked_add(1).ok_or_else(|| {
                PostgresPersistenceError::Invariant("HumanTask aggregate version overflowed".into())
            })?
        || !same_task_authority(existing, next)
        || next
            .task
            .expires_at
            .is_some_and(|expires_at| next.task.updated_at >= expires_at)
    {
        return Err(RepositoryError::Conflict(
            "HumanTask transition conflicts with stored state".into(),
        )
        .into());
    }
    let allowed = match (existing.task.status, next.task.status) {
        (HumanTaskStatus::PendingActivation, HumanTaskStatus::Ready) => true,
        (HumanTaskStatus::Ready, HumanTaskStatus::Claimed) => {
            next.task.claimed_by == Some(actor_principal_id)
        }
        (HumanTaskStatus::Claimed, HumanTaskStatus::Ready) => {
            existing.task.claimed_by == Some(actor_principal_id)
        }
        _ => false,
    };
    if actor_principal_id.as_uuid().is_nil() || !allowed || next.task.decision_id.is_some() {
        return Err(RepositoryError::Conflict(
            "HumanTask state transition is not an activation, claim, or release".into(),
        )
        .into());
    }
    Ok(())
}

fn validate_decision_transition(
    existing: &HumanTaskRecord,
    next: &HumanTaskDecisionRecord,
    expected_version: u64,
    actor_principal_id: PrincipalId,
) -> Result<(), PostgresPersistenceError> {
    if existing.task.aggregate_version != expected_version
        || next.task.task.aggregate_version
            != expected_version.checked_add(1).ok_or_else(|| {
                PostgresPersistenceError::Invariant("HumanTask aggregate version overflowed".into())
            })?
        || !same_task_authority(existing, &next.task)
        || existing.task.status.is_terminal()
        || !next.task.task.status.is_terminal()
        || next.decision.task_version != expected_version
        || next.task.task.decision_id != Some(next.decision.id)
        || actor_principal_id.as_uuid().is_nil()
        || next.decision.decided_by != actor_principal_id
    {
        return Err(RepositoryError::Conflict(
            "HumanTask decision conflicts with stored state".into(),
        )
        .into());
    }
    Ok(())
}

fn same_task_authority(left: &HumanTaskRecord, right: &HumanTaskRecord) -> bool {
    left.task.organization_id == right.task.organization_id
        && left.task.project_id == right.task.project_id
        && left.task.id == right.task.id
        && left.task.workflow_run_id == right.task.workflow_run_id
        && left.task.step_id == right.task.step_id
        && left.task.step_attempt == right.task.step_attempt
        && left.task.form_release == right.task.form_release
        && left.task.assignment_policy == right.task.assignment_policy
        && left.task.flow_run_id == right.task.flow_run_id
        && left.task.flow_hook_id == right.task.flow_hook_id
        && left.task.created_at == right.task.created_at
        && left.task.due_at == right.task.due_at
        && left.task.expires_at == right.task.expires_at
        && left.interaction == right.interaction
        && left.hook_event_sequence == right.hook_event_sequence
        && left.hook_event_id == right.hook_event_id
}

fn replay_task(
    existing: HumanTaskRecord,
    requested: &HumanTaskRecord,
) -> Result<IdempotentWrite<HumanTaskRecord>, PostgresPersistenceError> {
    if requested.task.status != HumanTaskStatus::PendingActivation
        || requested.task.aggregate_version != 1
        || requested.task.claimed_by.is_some()
        || requested.task.decision_id.is_some()
        || requested.interaction_request.is_some()
        || !same_task_authority(&existing, requested)
    {
        return Err(RepositoryError::Conflict(
            "Flow hook identity already created a different HumanTask".into(),
        )
        .into());
    }
    Ok(IdempotentWrite {
        value: existing,
        replayed: true,
    })
}

fn replay_task_collisions(
    mut existing: Vec<HumanTaskRecord>,
    requested: &HumanTaskRecord,
) -> Result<IdempotentWrite<HumanTaskRecord>, PostgresPersistenceError> {
    if existing.len() != 1 {
        return Err(RepositoryError::Conflict(
            "HumanTask identities conflict with existing tasks".into(),
        )
        .into());
    }
    let Some(existing) = existing.pop() else {
        return Err(PostgresPersistenceError::Invariant(
            "HumanTask collision resolution lost its selected task".into(),
        ));
    };
    replay_task(existing, requested)
}
