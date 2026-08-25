use super::attempt_postgres::{
    exact_attempt_where, load_attempt_for_update, PostgresConnectorExecutionAttemptRepository,
};
use super::evidence_postgres::insert_evidence_query;
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::connectors::domain::{
    ConnectorExecutionAttemptResolution, ConnectorExecutionAttemptResolutionReference,
    ConnectorExecutionAttemptState, IConnectorExecutionAttemptResolutionRepository,
    ResolveConnectorExecutionAttemptWrite,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IConnectorExecutionAttemptResolutionRepository
    for PostgresConnectorExecutionAttemptRepository
{
    async fn replay_resolution_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) = idempotency_replay::<
                        ConnectorExecutionAttemptResolutionReference,
                    >(transaction, &idempotency)
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_resolution(transaction, reference.value)
                        .await?
                        .map(Some)
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Connector attempt resolution replay fact is missing".into(),
                            )
                        })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn resolve_indeterminate(
        &self,
        write: ResolveConnectorExecutionAttemptWrite,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptResolution>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<
                        ConnectorExecutionAttemptResolutionReference,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_resolution(transaction, reference.value)
                                .await?
                                .ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "Connector attempt resolution replay fact is missing".into(),
                                    )
                                })?,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = load_attempt_for_update(transaction, write.resolution.binding())
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    // A concurrent identical request can commit while this transaction waits for
                    // the exact attempt row. Recheck under that serialization fence so it replays
                    // the committed result instead of observing only the terminal attempt.
                    if let Some(reference) = idempotency_replay::<
                        ConnectorExecutionAttemptResolutionReference,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_resolution(transaction, reference.value)
                                .await?
                                .ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "Connector attempt resolution replay fact is missing"
                                            .into(),
                                    )
                                })?,
                            replayed: true,
                        });
                    }
                    if current.state() == ConnectorExecutionAttemptState::Terminal {
                        return Err(RepositoryError::Conflict(
                            "Connector execution attempt is already terminal".into(),
                        )
                        .into());
                    }
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    let insertion = execute(transaction, insert_resolution_query(&write.resolution))
                        .await;
                    match insertion {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "resolving Connector execution attempt affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Connector execution attempt is already resolved".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let evidence_insertion =
                        execute(transaction, insert_evidence_query(&write.evidence)).await;
                    match evidence_insertion {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "recording Connector indeterminate evidence affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Connector execution attempt already records terminal evidence"
                                    .into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    require_one_row(
                        "Connector execution attempt resolution",
                        execute(
                            transaction,
                            exact_attempt_where(
                                sql_query::<()>(
                                    "update connector_execution_attempts set state = 'terminal', terminal_at = ",
                                )
                                .bind(write.resolution.resolved_at()),
                                write.resolution.binding(),
                            )
                            .append(" and state = 'dispatching' and dispatch_started_at = ")
                            .bind(write.resolution.dispatch_started_at())
                            .append(" and outcome_deadline_at = ")
                            .bind(write.resolution.outcome_deadline_at()),
                        )
                        .await?,
                    )?;
                    store_outbox(transaction, &write.event).await?;
                    let binding = write.resolution.binding();
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: binding.organization_id().as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "connector.execution-attempt.resolved",
                            aggregate_id: binding.attempt_id(),
                            occurred_at: write.resolution.resolved_at(),
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                binding.project_id(),
                                Some(binding.environment_id()),
                            ),
                            details: serde_json::json!({
                                "projectId": binding.project_id(),
                                "environmentId": binding.environment_id(),
                                "profileId": binding.profile_id(),
                                "revisionId": binding.revision_id(),
                                "attemptId": binding.attempt_id(),
                                "requestDigest": binding.request_digest().as_str(),
                                "requestBodyBytes": binding.request_body_bytes(),
                                "resolution": "indeterminate",
                                "reason": write.resolution.reason(),
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(
                        transaction,
                        &write.idempotency,
                        &ConnectorExecutionAttemptResolutionReference::from(&write.resolution),
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.resolution,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_resolution(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                resolution_select()
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
            .await
            .map_err(storage)?
            .map(decode_resolution)
            .transpose()
    }
}

struct ConnectorExecutionAttemptResolutionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    revision_id: Uuid,
    attempt_id: Uuid,
    request_digest: String,
    request_body_bytes: u64,
    dispatch_started_at: DateTime<Utc>,
    outcome_deadline_at: DateTime<Utc>,
    reason: String,
    resolved_by: Uuid,
    resolved_at: DateTime<Utc>,
}

impl FromRow for ConnectorExecutionAttemptResolutionRow {
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
            dispatch_started_at: decode(row, 8)?,
            outcome_deadline_at: decode(row, 9)?,
            reason: decode(row, 10)?,
            resolved_by: decode(row, 11)?,
            resolved_at: decode(row, 12)?,
        })
    }
}

fn resolution_select() -> a3s_orm::SqlQuery<ConnectorExecutionAttemptResolutionRow> {
    sql_query::<ConnectorExecutionAttemptResolutionRow>(
        "select organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, dispatch_started_at, outcome_deadline_at, reason, resolved_by, resolved_at from connector_execution_attempt_resolutions",
    )
}

async fn load_resolution(
    transaction: &PostgresTransaction,
    reference: ConnectorExecutionAttemptResolutionReference,
) -> Result<Option<ConnectorExecutionAttemptResolution>, PostgresPersistenceError> {
    fetch_optional::<ConnectorExecutionAttemptResolutionRow, _>(
        transaction,
        resolution_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(reference.profile_id.as_uuid())
            .append(" and revision_id = ")
            .bind(reference.revision_id.as_uuid())
            .append(" and attempt_id = ")
            .bind(reference.attempt_id),
    )
    .await?
    .map(decode_resolution)
    .transpose()
    .map_err(PostgresPersistenceError::Repository)
}

fn decode_resolution(
    row: ConnectorExecutionAttemptResolutionRow,
) -> Result<ConnectorExecutionAttemptResolution, RepositoryError> {
    ConnectorExecutionAttemptResolution::restore(
        crate::modules::connectors::domain::ConnectorExecutionAttemptBinding::restore(
            OrganizationId::from_uuid(row.organization_id),
            ProjectId::from_uuid(row.project_id),
            EnvironmentId::from_uuid(row.environment_id),
            ConnectorProfileId::from_uuid(row.profile_id),
            ConnectorRevisionId::from_uuid(row.revision_id),
            row.attempt_id,
            Sha256Digest::parse(row.request_digest).map_err(stored("resolution digest"))?,
            row.request_body_bytes,
        )
        .map_err(stored("Connector attempt resolution binding"))?,
        row.dispatch_started_at,
        row.outcome_deadline_at,
        row.reason,
        PrincipalId::from_uuid(row.resolved_by),
        row.resolved_at,
    )
    .map_err(stored("Connector execution attempt resolution"))
}

fn insert_resolution_query(
    resolution: &ConnectorExecutionAttemptResolution,
) -> a3s_orm::SqlQuery<()> {
    let binding = resolution.binding();
    sql_query::<()>("insert into connector_execution_attempt_resolutions (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, dispatch_started_at, outcome_deadline_at, reason, resolved_by, resolved_at) values (")
        .bind(binding.organization_id().as_uuid())
        .append(", ")
        .bind(binding.project_id().as_uuid())
        .append(", ")
        .bind(binding.environment_id().as_uuid())
        .append(", ")
        .bind(binding.profile_id().as_uuid())
        .append(", ")
        .bind(binding.revision_id().as_uuid())
        .append(", ")
        .bind(binding.attempt_id())
        .append(", ")
        .bind(binding.request_digest().as_str())
        .append(", ")
        .bind(binding.request_body_bytes())
        .append(", ")
        .bind(resolution.dispatch_started_at())
        .append(", ")
        .bind(resolution.outcome_deadline_at())
        .append(", ")
        .bind(resolution.reason().to_owned())
        .append(", ")
        .bind(resolution.resolved_by().as_uuid())
        .append(", ")
        .bind(resolution.resolved_at())
        .append(")")
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
