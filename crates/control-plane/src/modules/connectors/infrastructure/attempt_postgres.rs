use super::evidence_postgres::{
    decode_evidence, evidence_select, insert_evidence_query, ConnectorExecutionEvidenceRow,
};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::connectors::domain::{
    reservation_record, BeginConnectorExecutionDispatch, ConnectorExecutionAttempt,
    ConnectorExecutionAttemptBinding, ConnectorExecutionAttemptCursor,
    ConnectorExecutionAttemptRecord, ConnectorExecutionAttemptState, ConnectorExecutionOutcome,
    ConnectorExecutionReservation, ConnectorRevisionRevocation,
    ConnectorRevisionRevocationReference, IConnectorExecutionAttemptRepository,
    IConnectorRevisionRevocationRepository, ReserveConnectorExecutionAttempt,
    RevokeConnectorRevisionWrite, SettleConnectorExecutionAttempt,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_ATTEMPTS: &str = "select organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, state, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, terminal_at, created_at from connector_execution_attempts";

#[derive(Clone)]
pub struct PostgresConnectorExecutionAttemptRepository {
    executor: PostgresExecutor,
}

impl PostgresConnectorExecutionAttemptRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IConnectorExecutionAttemptRepository for PostgresConnectorExecutionAttemptRepository {
    async fn reserve(
        &self,
        request: ReserveConnectorExecutionAttempt,
    ) -> Result<ConnectorExecutionReservation, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    request
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let mut current = load_attempt_for_update(transaction, &request.binding).await?;
                    if current.is_none() {
                        let insertion = execute(
                            transaction,
                            insert_reservation_query(&request)
                                .append(" on conflict (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id) do nothing"),
                        )
                        .await;
                        let inserted = match insertion {
                            Ok(rows) => rows,
                            Err(error) if is_foreign_key_violation(&error) => {
                                return Err(RepositoryError::NotFound.into())
                            }
                            Err(error) => return Err(error),
                        };
                        match inserted {
                            1 => {
                                let record = reservation_record(&request, 1, request.reserved_at)
                                    .map_err(PostgresPersistenceError::Invariant)?;
                                return Ok(ConnectorExecutionReservation::Acquired {
                                    fence: record.attempt.fence(),
                                    replayed: false,
                                });
                            }
                            0 => {
                                current = load_attempt_for_update(transaction, &request.binding).await?;
                            }
                            rows => {
                                return Err(PostgresPersistenceError::Invariant(format!(
                                    "reserving Connector execution attempt affected {rows} rows"
                                )))
                            }
                        }
                    }
                    reserve_existing(
                        transaction,
                        request,
                        current.ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "conflicting Connector execution reservation is missing".into(),
                            )
                        })?,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn begin_dispatch(
        &self,
        request: BeginConnectorExecutionDispatch,
    ) -> Result<ConnectorExecutionAttemptRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    request
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let revision = lock_exact_revision(
                        transaction,
                        request.fence.binding().organization_id(),
                        request.fence.binding().project_id(),
                        request.fence.binding().environment_id(),
                        request.fence.binding().profile_id(),
                        request.fence.binding().revision_id(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if load_revocation(
                        transaction,
                        ConnectorRevisionRevocationReference {
                            organization_id: request.fence.binding().organization_id(),
                            project_id: request.fence.binding().project_id(),
                            environment_id: request.fence.binding().environment_id(),
                            profile_id: request.fence.binding().profile_id(),
                            revision_id: request.fence.binding().revision_id(),
                        },
                    )
                    .await?
                    .is_some()
                    {
                        return Err(revision_revoked().into());
                    }
                    debug_assert_eq!(revision.id, request.fence.binding().revision_id().as_uuid());
                    let current = load_attempt_for_update(transaction, request.fence.binding())
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if current.state() != ConnectorExecutionAttemptState::Reserved
                        || current.fence() != request.fence
                    {
                        return Err(fence_conflict().into());
                    }
                    let attempt = ConnectorExecutionAttempt::restore(
                        current.binding().clone(),
                        ConnectorExecutionAttemptState::Dispatching,
                        current.fence_generation(),
                        request.fence.token(),
                        current.reserved_at(),
                        current.lease_expires_at(),
                        Some(request.dispatch_started_at),
                        Some(request.outcome_deadline_at),
                        None,
                        current.created_at(),
                    )
                    .map_err(PostgresPersistenceError::Invariant)?;
                    require_one_row(
                        "Connector execution dispatch",
                        execute(
                            transaction,
                            exact_attempt_where(
                                sql_query::<()>("update connector_execution_attempts set state = 'dispatching', dispatch_started_at = ")
                                    .bind(request.dispatch_started_at)
                                    .append(", outcome_deadline_at = ")
                                    .bind(request.outcome_deadline_at),
                                request.fence.binding(),
                            )
                                .append(" and state = 'reserved' and fence_generation = ")
                                .bind(request.fence.generation())
                                .append(" and fence_token = ")
                                .bind(request.fence.token()),
                        )
                        .await?,
                    )?;
                    ConnectorExecutionAttemptRecord::new(attempt, None)
                        .map_err(PostgresPersistenceError::Invariant)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn settle(
        &self,
        request: SettleConnectorExecutionAttempt,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    request
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = load_attempt_for_update(transaction, request.fence.binding())
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if current.state() == ConnectorExecutionAttemptState::Terminal {
                        let record = record_from_attempt(transaction, current).await?;
                        if record.attempt.fence() == request.fence
                            && record.evidence.as_ref() == Some(&request.evidence)
                        {
                            return Ok(IdempotentWrite {
                                value: record,
                                replayed: true,
                            });
                        }
                        return Err(evidence_conflict().into());
                    }
                    if current.fence() != request.fence {
                        return Err(fence_conflict().into());
                    }
                    let (dispatch_started_at, outcome_deadline_at) = match current.state() {
                        ConnectorExecutionAttemptState::Reserved => {
                            if request.evidence.outcome() == ConnectorExecutionOutcome::Accepted
                                || request.evidence.response_status().is_some()
                                || request.evidence.started_at() != current.reserved_at()
                                || request.evidence.completed_at() > current.lease_expires_at()
                            {
                                return Err(RepositoryError::Conflict(
                                    "Connector pre-dispatch settlement claims a provider response"
                                        .into(),
                                )
                                .into());
                            }
                            (None, None)
                        }
                        ConnectorExecutionAttemptState::Dispatching => {
                            if request.evidence.started_at()
                                != current
                                    .dispatch_started_at()
                                    .expect("validated Connector dispatch")
                            {
                                return Err(RepositoryError::Conflict(
                                    "Connector execution evidence does not match dispatch start"
                                        .into(),
                                )
                                .into());
                            }
                            (
                                current.dispatch_started_at(),
                                current.outcome_deadline_at(),
                            )
                        }
                        ConnectorExecutionAttemptState::Terminal => unreachable!("handled above"),
                    };
                    let insertion = execute(transaction, insert_evidence_query(&request.evidence))
                        .await;
                    match insertion {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "recording fenced Connector evidence affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(evidence_conflict().into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    require_one_row(
                        "Connector execution settlement",
                        execute(
                            transaction,
                            exact_attempt_where(
                                sql_query::<()>("update connector_execution_attempts set state = 'terminal', terminal_at = ")
                                    .bind(request.evidence.completed_at()),
                                request.fence.binding(),
                            )
                                .append(" and state = ")
                                .bind(current.state().as_str())
                                .append(" and fence_generation = ")
                                .bind(request.fence.generation())
                                .append(" and fence_token = ")
                                .bind(request.fence.token()),
                        )
                        .await?,
                    )?;
                    let attempt = ConnectorExecutionAttempt::restore(
                        current.binding().clone(),
                        ConnectorExecutionAttemptState::Terminal,
                        current.fence_generation(),
                        request.fence.token(),
                        current.reserved_at(),
                        current.lease_expires_at(),
                        dispatch_started_at,
                        outcome_deadline_at,
                        Some(request.evidence.completed_at()),
                        current.created_at(),
                    )
                    .map_err(PostgresPersistenceError::Invariant)?;
                    let record = ConnectorExecutionAttemptRecord::new(
                        attempt,
                        Some(request.evidence),
                    )
                    .map_err(PostgresPersistenceError::Invariant)?;
                    Ok(IdempotentWrite {
                        value: record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(attempt) = fetch_optional::<ConnectorExecutionAttemptRow, _>(
                        transaction,
                        attempt_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" and environment_id = ")
                            .bind(environment_id.as_uuid())
                            .append(" and profile_id = ")
                            .bind(profile_id.as_uuid())
                            .append(" and revision_id = ")
                            .bind(revision_id.as_uuid())
                            .append(" and attempt_id = ")
                            .bind(attempt_id),
                    )
                    .await?
                    .map(decode_attempt)
                    .transpose()?
                    else {
                        return Ok(None);
                    };
                    record_from_attempt(transaction, attempt).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_unresolved_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionAttemptCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionAttemptRecord>, RepositoryError> {
        let after = after
            .map(ConnectorExecutionAttemptCursor::validate)
            .transpose()
            .map_err(RepositoryError::Storage)?;
        let mut query = attempt_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(profile_id.as_uuid())
            .append(" and revision_id = ")
            .bind(revision_id.as_uuid())
            .append(" and state <> 'terminal'");
        if let Some(cursor) = after {
            query = query
                .append(" and (created_at < ")
                .bind(cursor.created_at)
                .append(" or (created_at = ")
                .bind(cursor.created_at)
                .append(" and attempt_id < ")
                .bind(cursor.attempt_id)
                .append("))");
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .append(" order by created_at desc, attempt_id desc limit ")
                    .bind(limit.clamp(1, MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE + 1)),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_attempt)
            .map(|attempt| {
                attempt.and_then(|attempt| {
                    ConnectorExecutionAttemptRecord::new(attempt, None)
                        .map_err(stored("Connector execution attempt record"))
                })
            })
            .collect()
    }
}

#[async_trait]
impl IConnectorRevisionRevocationRepository for PostgresConnectorExecutionAttemptRepository {
    async fn replay_revocation_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorRevisionRevocation>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<ConnectorRevisionRevocationReference>(
                            transaction,
                            &idempotency,
                        )
                        .await?
                    else {
                        return Ok(None);
                    };
                    load_revocation(transaction, reference.value)
                        .await?
                        .map(Some)
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Connector revocation replay fact is missing".into(),
                            )
                        })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_revision(
        &self,
        write: RevokeConnectorRevisionWrite,
    ) -> Result<IdempotentWrite<ConnectorRevisionRevocation>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<
                        ConnectorRevisionRevocationReference,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_revocation(transaction, reference.value)
                                .await?
                                .ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "Connector revocation replay fact is missing".into(),
                                    )
                                })?,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let exact = lock_exact_revision(
                        transaction,
                        write.revocation.organization_id,
                        write.revocation.project_id,
                        write.revocation.environment_id,
                        write.revocation.profile_id,
                        write.revocation.revision_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if exact.revision_number != write.revocation.revision_number
                        || exact.definition_digest != write.revocation.definition_digest.as_str()
                        || write.revocation.revoked_at < exact.created_at
                    {
                        return Err(RepositoryError::Conflict(
                            "Connector revision revocation authority drifted".into(),
                        )
                        .into());
                    }
                    let insertion =
                        execute(transaction, insert_revocation_query(&write.revocation)).await;
                    match insertion {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "revoking Connector revision affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Connector revision is already revoked".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: write.revocation.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "connector.revision.revoked",
                            aggregate_id: write.revocation.revision_id.as_uuid(),
                            occurred_at: write.revocation.revoked_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                write.revocation.project_id,
                                Some(write.revocation.environment_id),
                            ),
                            details: serde_json::json!({
                                "projectId": write.revocation.project_id,
                                "environmentId": write.revocation.environment_id,
                                "profileId": write.revocation.profile_id,
                                "revisionId": write.revocation.revision_id,
                                "revisionNumber": write.revocation.revision_number,
                                "definitionDigest": write.revocation.definition_digest.as_str(),
                                "reason": write.revocation.reason.as_str(),
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(
                        transaction,
                        &write.idempotency,
                        &ConnectorRevisionRevocationReference::from(&write.revocation),
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.revocation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision_revocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Option<ConnectorRevisionRevocation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revocation_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(profile_id.as_uuid())
                    .append(" and revision_id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_revocation)
            .transpose()
    }
}

struct ConnectorRevisionAuthorityRow {
    id: Uuid,
    revision_number: u64,
    definition_digest: String,
    created_at: DateTime<Utc>,
}

impl FromRow for ConnectorRevisionAuthorityRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            revision_number: decode(row, 1)?,
            definition_digest: decode(row, 2)?,
            created_at: decode(row, 3)?,
        })
    }
}

async fn lock_exact_revision(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
) -> Result<Option<ConnectorRevisionAuthorityRow>, PostgresPersistenceError> {
    fetch_optional::<ConnectorRevisionAuthorityRow, _>(
        transaction,
        sql_query::<ConnectorRevisionAuthorityRow>(
            "select id, revision_number, definition_digest, created_at from connector_revisions where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(project_id.as_uuid())
        .append(" and environment_id = ")
        .bind(environment_id.as_uuid())
        .append(" and profile_id = ")
        .bind(profile_id.as_uuid())
        .append(" and id = ")
        .bind(revision_id.as_uuid())
        .append(" for update"),
    )
    .await
}

struct ConnectorRevisionRevocationRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    revision_id: Uuid,
    revision_number: u64,
    definition_digest: String,
    reason: String,
    revoked_by: Uuid,
    revoked_at: DateTime<Utc>,
}

impl FromRow for ConnectorRevisionRevocationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            profile_id: decode(row, 3)?,
            revision_id: decode(row, 4)?,
            revision_number: decode(row, 5)?,
            definition_digest: decode(row, 6)?,
            reason: decode(row, 7)?,
            revoked_by: decode(row, 8)?,
            revoked_at: decode(row, 9)?,
        })
    }
}

fn revocation_select() -> a3s_orm::SqlQuery<ConnectorRevisionRevocationRow> {
    sql_query::<ConnectorRevisionRevocationRow>(
        "select organization_id, project_id, environment_id, profile_id, revision_id, revision_number, definition_digest, reason, revoked_by, revoked_at from connector_revision_revocations",
    )
}

async fn load_revocation(
    transaction: &PostgresTransaction,
    reference: ConnectorRevisionRevocationReference,
) -> Result<Option<ConnectorRevisionRevocation>, PostgresPersistenceError> {
    fetch_optional::<ConnectorRevisionRevocationRow, _>(
        transaction,
        revocation_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(reference.profile_id.as_uuid())
            .append(" and revision_id = ")
            .bind(reference.revision_id.as_uuid()),
    )
    .await?
    .map(decode_revocation)
    .transpose()
    .map_err(PostgresPersistenceError::Repository)
}

fn decode_revocation(
    row: ConnectorRevisionRevocationRow,
) -> Result<ConnectorRevisionRevocation, RepositoryError> {
    ConnectorRevisionRevocation::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.environment_id),
        ConnectorProfileId::from_uuid(row.profile_id),
        ConnectorRevisionId::from_uuid(row.revision_id),
        row.revision_number,
        Sha256Digest::parse(row.definition_digest).map_err(stored("revocation digest"))?,
        row.reason,
        PrincipalId::from_uuid(row.revoked_by),
        row.revoked_at,
    )
    .map_err(stored("Connector revision revocation"))
}

fn insert_revocation_query(revocation: &ConnectorRevisionRevocation) -> a3s_orm::SqlQuery<()> {
    sql_query::<()>("insert into connector_revision_revocations (organization_id, project_id, environment_id, profile_id, revision_id, revision_number, definition_digest, reason, revoked_by, revoked_at) values (")
        .bind(revocation.organization_id.as_uuid())
        .append(", ")
        .bind(revocation.project_id.as_uuid())
        .append(", ")
        .bind(revocation.environment_id.as_uuid())
        .append(", ")
        .bind(revocation.profile_id.as_uuid())
        .append(", ")
        .bind(revocation.revision_id.as_uuid())
        .append(", ")
        .bind(revocation.revision_number)
        .append(", ")
        .bind(revocation.definition_digest.as_str())
        .append(", ")
        .bind(revocation.reason.clone())
        .append(", ")
        .bind(revocation.revoked_by.as_uuid())
        .append(", ")
        .bind(revocation.revoked_at)
        .append(")")
}

async fn reserve_existing(
    transaction: &PostgresTransaction,
    request: ReserveConnectorExecutionAttempt,
    current: ConnectorExecutionAttempt,
) -> Result<ConnectorExecutionReservation, PostgresPersistenceError> {
    if current.binding() != &request.binding {
        return Err(request_conflict().into());
    }
    match current.state() {
        ConnectorExecutionAttemptState::Terminal => Ok(ConnectorExecutionReservation::Completed(
            record_from_attempt(transaction, current).await?,
        )),
        ConnectorExecutionAttemptState::Dispatching => {
            let record = ConnectorExecutionAttemptRecord::new(current.clone(), None)
                .map_err(PostgresPersistenceError::Invariant)?;
            match current.recovery_state(request.reserved_at) {
                crate::modules::connectors::domain::ConnectorExecutionRecoveryState::InFlight => {
                    Ok(ConnectorExecutionReservation::InFlight(record))
                }
                crate::modules::connectors::domain::ConnectorExecutionRecoveryState::Indeterminate => {
                    Ok(ConnectorExecutionReservation::Indeterminate(record))
                }
                _ => Err(PostgresPersistenceError::Invariant(
                    "stored Connector dispatch recovery state is invalid".into(),
                )),
            }
        }
        ConnectorExecutionAttemptState::Reserved => {
            let current_fence = current.fence();
            if current_fence.token() == request.fence_token {
                if current_fence.reserved_at() == request.reserved_at
                    && current_fence.lease_expires_at() == request.lease_expires_at
                    && request.reserved_at < current_fence.lease_expires_at()
                {
                    return Ok(ConnectorExecutionReservation::Acquired {
                        fence: current_fence,
                        replayed: true,
                    });
                }
                return Err(fence_conflict().into());
            }
            if request.reserved_at < current.lease_expires_at() {
                return Ok(ConnectorExecutionReservation::Busy(
                    ConnectorExecutionAttemptRecord::new(current, None)
                        .map_err(PostgresPersistenceError::Invariant)?,
                ));
            }
            let generation = current.fence_generation().checked_add(1).ok_or_else(|| {
                PostgresPersistenceError::Repository(RepositoryError::Conflict(
                    "Connector execution fence generation overflowed".into(),
                ))
            })?;
            require_one_row(
                "Connector execution reservation takeover",
                execute(
                    transaction,
                    exact_attempt_where(
                        sql_query::<()>(
                            "update connector_execution_attempts set fence_generation = ",
                        )
                        .bind(generation)
                        .append(", fence_token = ")
                        .bind(request.fence_token)
                        .append(", reserved_at = ")
                        .bind(request.reserved_at)
                        .append(", lease_expires_at = ")
                        .bind(request.lease_expires_at),
                        &request.binding,
                    )
                    .append(" and state = 'reserved' and fence_generation = ")
                    .bind(current.fence_generation())
                    .append(" and fence_token = ")
                    .bind(current.fence().token()),
                )
                .await?,
            )?;
            let record = reservation_record(&request, generation, current.created_at())
                .map_err(PostgresPersistenceError::Invariant)?;
            Ok(ConnectorExecutionReservation::Acquired {
                fence: record.attempt.fence(),
                replayed: false,
            })
        }
    }
}

struct ConnectorExecutionAttemptRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    revision_id: Uuid,
    attempt_id: Uuid,
    request_digest: String,
    request_body_bytes: u64,
    state: String,
    fence_generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    dispatch_started_at: Option<DateTime<Utc>>,
    outcome_deadline_at: Option<DateTime<Utc>>,
    terminal_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl FromRow for ConnectorExecutionAttemptRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            profile_id: decode(row, 3)?,
            revision_id: decode(row, 4)?,
            attempt_id: decode(row, 5)?,
            request_digest: decode(row, 6)?,
            request_body_bytes: decode(row, 7)?,
            state: decode(row, 8)?,
            fence_generation: decode(row, 9)?,
            fence_token: decode(row, 10)?,
            reserved_at: decode(row, 11)?,
            lease_expires_at: decode(row, 12)?,
            dispatch_started_at: decode(row, 13)?,
            outcome_deadline_at: decode(row, 14)?,
            terminal_at: decode(row, 15)?,
            created_at: decode(row, 16)?,
        })
    }
}

fn attempt_select() -> a3s_orm::SqlQuery<ConnectorExecutionAttemptRow> {
    sql_query::<ConnectorExecutionAttemptRow>(SELECT_ATTEMPTS)
}

fn insert_reservation_query(request: &ReserveConnectorExecutionAttempt) -> a3s_orm::SqlQuery<()> {
    sql_query::<()>("insert into connector_execution_attempts (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, state, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, terminal_at, created_at) values (")
        .bind(request.binding.organization_id().as_uuid())
        .append(", ")
        .bind(request.binding.project_id().as_uuid())
        .append(", ")
        .bind(request.binding.environment_id().as_uuid())
        .append(", ")
        .bind(request.binding.profile_id().as_uuid())
        .append(", ")
        .bind(request.binding.revision_id().as_uuid())
        .append(", ")
        .bind(request.binding.attempt_id())
        .append(", ")
        .bind(request.binding.request_digest().as_str())
        .append(", ")
        .bind(request.binding.request_body_bytes())
        .append(", 'reserved', 1, ")
        .bind(request.fence_token)
        .append(", ")
        .bind(request.reserved_at)
        .append(", ")
        .bind(request.lease_expires_at)
        .append(", null, null, null, ")
        .bind(request.reserved_at)
        .append(")")
}

fn exact_attempt_where(
    query: a3s_orm::SqlQuery<()>,
    binding: &ConnectorExecutionAttemptBinding,
) -> a3s_orm::SqlQuery<()> {
    query
        .append(" where organization_id = ")
        .bind(binding.organization_id().as_uuid())
        .append(" and project_id = ")
        .bind(binding.project_id().as_uuid())
        .append(" and environment_id = ")
        .bind(binding.environment_id().as_uuid())
        .append(" and profile_id = ")
        .bind(binding.profile_id().as_uuid())
        .append(" and revision_id = ")
        .bind(binding.revision_id().as_uuid())
        .append(" and attempt_id = ")
        .bind(binding.attempt_id())
}

async fn load_attempt_for_update(
    transaction: &PostgresTransaction,
    binding: &ConnectorExecutionAttemptBinding,
) -> Result<Option<ConnectorExecutionAttempt>, PostgresPersistenceError> {
    fetch_optional::<ConnectorExecutionAttemptRow, _>(
        transaction,
        attempt_select()
            .append(" where organization_id = ")
            .bind(binding.organization_id().as_uuid())
            .append(" and project_id = ")
            .bind(binding.project_id().as_uuid())
            .append(" and environment_id = ")
            .bind(binding.environment_id().as_uuid())
            .append(" and profile_id = ")
            .bind(binding.profile_id().as_uuid())
            .append(" and revision_id = ")
            .bind(binding.revision_id().as_uuid())
            .append(" and attempt_id = ")
            .bind(binding.attempt_id())
            .append(" for update"),
    )
    .await?
    .map(decode_attempt)
    .transpose()
    .map_err(PostgresPersistenceError::Repository)
}

async fn record_from_attempt(
    transaction: &PostgresTransaction,
    attempt: ConnectorExecutionAttempt,
) -> Result<ConnectorExecutionAttemptRecord, PostgresPersistenceError> {
    let evidence = if attempt.state() == ConnectorExecutionAttemptState::Terminal {
        Some(
            fetch_optional::<ConnectorExecutionEvidenceRow, _>(
                transaction,
                evidence_select()
                    .append(" where organization_id = ")
                    .bind(attempt.binding().organization_id().as_uuid())
                    .append(" and project_id = ")
                    .bind(attempt.binding().project_id().as_uuid())
                    .append(" and environment_id = ")
                    .bind(attempt.binding().environment_id().as_uuid())
                    .append(" and profile_id = ")
                    .bind(attempt.binding().profile_id().as_uuid())
                    .append(" and revision_id = ")
                    .bind(attempt.binding().revision_id().as_uuid())
                    .append(" and attempt_id = ")
                    .bind(attempt.binding().attempt_id()),
            )
            .await?
            .map(decode_evidence)
            .transpose()?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "terminal Connector execution attempt evidence is missing".into(),
                )
            })?,
        )
    } else {
        None
    };
    ConnectorExecutionAttemptRecord::new(attempt, evidence)
        .map_err(PostgresPersistenceError::Invariant)
}

fn decode_attempt(
    row: ConnectorExecutionAttemptRow,
) -> Result<ConnectorExecutionAttempt, RepositoryError> {
    ConnectorExecutionAttempt::restore(
        ConnectorExecutionAttemptBinding::restore(
            OrganizationId::from_uuid(row.organization_id),
            ProjectId::from_uuid(row.project_id),
            EnvironmentId::from_uuid(row.environment_id),
            ConnectorProfileId::from_uuid(row.profile_id),
            ConnectorRevisionId::from_uuid(row.revision_id),
            row.attempt_id,
            Sha256Digest::parse(row.request_digest)
                .map_err(stored("Connector execution request digest"))?,
            row.request_body_bytes,
        )
        .map_err(stored("Connector execution attempt binding"))?,
        ConnectorExecutionAttemptState::parse(&row.state)
            .map_err(stored("Connector execution attempt state"))?,
        row.fence_generation,
        row.fence_token,
        row.reserved_at,
        row.lease_expires_at,
        row.dispatch_started_at,
        row.outcome_deadline_at,
        row.terminal_at,
        row.created_at,
    )
    .map_err(stored("Connector execution attempt"))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn request_conflict() -> RepositoryError {
    RepositoryError::Conflict(
        "Connector execution attempt identity is already bound to another request".into(),
    )
}

fn fence_conflict() -> RepositoryError {
    RepositoryError::Conflict("Connector execution fence is stale or ambiguous".into())
}

fn revision_revoked() -> RepositoryError {
    RepositoryError::Forbidden("Connector revision was revoked before dispatch".into())
}

fn evidence_conflict() -> RepositoryError {
    RepositoryError::Conflict(
        "Connector execution attempt already records a different terminal fact".into(),
    )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
