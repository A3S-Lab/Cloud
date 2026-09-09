use crate::infrastructure::{
    execute, fetch_all, fetch_optional, transaction_error, PostgresPersistenceError,
};
use crate::modules::inference::domain::{
    apply_inference_usage_batch, project_inserted_usage_records, AcceptInferenceUsageBatchWrite,
    IInferenceUsageRepository, InferenceUsageDailyRollup, InferenceUsageDailyRollupKey,
    InferenceUsageLedgerError, InferenceUsageLedgerState, InferenceUsageRequestFact,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, RepositoryError};
use a3s_cloud_contracts::{
    InferenceUsageCursorV1, InferenceUsageEndpointV1, InferenceUsageMeasurementCompletenessV1,
    InferenceUsageReceiptV1, InferenceUsageTerminalOutcomeV1,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

struct WatermarkRow {
    boot_epoch: Option<Uuid>,
    sequence: Option<i64>,
}

impl FromRow for WatermarkRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            boot_epoch: decode(row, 0)?,
            sequence: decode(row, 1)?,
        })
    }
}

impl WatermarkRow {
    fn cursor(&self) -> Result<Option<InferenceUsageCursorV1>, RepositoryError> {
        match (self.boot_epoch, self.sequence) {
            (None, None) => Ok(None),
            (Some(boot_epoch), Some(sequence)) if sequence > 0 => Ok(Some(InferenceUsageCursorV1 {
                boot_epoch,
                sequence: u64::try_from(sequence).map_err(|_| {
                    RepositoryError::Storage(
                        "inference usage watermark sequence exceeds u64".into(),
                    )
                })?,
            })),
            _ => Err(RepositoryError::Storage(
                "inference usage watermark row is inconsistent".into(),
            )),
        }
    }
}

struct EventDigestRow {
    event_id: Uuid,
    payload_sha256: String,
}

impl FromRow for EventDigestRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            event_id: decode(row, 0)?,
            payload_sha256: decode(row, 1)?,
        })
    }
}

fn decode<T>(row: &impl Row, index: usize) -> Result<T, DecodeError>
where
    T: FromValue,
{
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn ledger_error(error: InferenceUsageLedgerError) -> RepositoryError {
    match error {
        InferenceUsageLedgerError::Conflict(message) => RepositoryError::Conflict(message),
        InferenceUsageLedgerError::Contract(message) => RepositoryError::Storage(message),
    }
}

/// Durable Inference usage ledger backed by PostgreSQL.
#[derive(Clone)]
pub struct PostgresInferenceUsageRepository {
    executor: PostgresExecutor,
}

impl PostgresInferenceUsageRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IInferenceUsageRepository for PostgresInferenceUsageRepository {
    async fn accept_usage_batch(
        &self,
        write: AcceptInferenceUsageBatchWrite,
    ) -> Result<InferenceUsageReceiptV1, RepositoryError> {
        write
            .validate()
            .map_err(|error| RepositoryError::Storage(error))?;
        let organization_id = write.organization_id.as_uuid();
        let gateway_id = write.batch.gateway_id;
        let accepted_at = write.accepted_at;
        let batch = write.batch;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into inference_usage_watermarks (organization_id, gateway_id, boot_epoch, sequence, updated_at) values (",
                        )
                        .bind(organization_id)
                        .append(", ")
                        .bind(gateway_id)
                        .append(", null, null, ")
                        .bind(accepted_at)
                        .append(") on conflict (organization_id, gateway_id) do nothing"),
                    )
                    .await?;

                    let watermark = fetch_optional::<WatermarkRow, _>(
                        transaction,
                        sql_query::<WatermarkRow>(
                            "select boot_epoch, sequence from inference_usage_watermarks where organization_id = ",
                        )
                        .bind(organization_id)
                        .append(" and gateway_id = ")
                        .bind(gateway_id)
                        .append(" for update"),
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "inference usage watermark row missing after ensure".into(),
                        )
                    })?;

                    let mut events = HashMap::new();
                    if !batch.records.is_empty() {
                        let mut query = sql_query::<EventDigestRow>(
                            "select event_id, payload_sha256 from inference_usage_events where organization_id = ",
                        )
                        .bind(organization_id)
                        .append(" and gateway_id = ")
                        .bind(gateway_id)
                        .append(" and event_id in (");
                        for (index, record) in batch.records.iter().enumerate() {
                            if index > 0 {
                                query = query.append(", ");
                            }
                            query = query.bind(record.event_id);
                        }
                        query = query.append(")");
                        for row in fetch_all::<EventDigestRow, _>(transaction, query).await? {
                            events.insert(row.event_id, row.payload_sha256);
                        }
                    }

                    let state = InferenceUsageLedgerState {
                        watermark: watermark.cursor()?,
                        events,
                    };
                    let applied =
                        apply_inference_usage_batch(&state, &batch).map_err(ledger_error)?;

                    if applied.next == state {
                        return Ok::<_, PostgresPersistenceError>(applied.receipt);
                    }

                    for record in &batch.records {
                        if !applied.inserted_event_ids.contains(&record.event_id) {
                            continue;
                        }
                        let payload = record.payload().map_err(|error| {
                            RepositoryError::Storage(error)
                        })?;
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into inference_usage_events (organization_id, gateway_id, event_id, payload_sha256, payload, boot_epoch, sequence, batch_id, accepted_at) values (",
                            )
                            .bind(organization_id)
                            .append(", ")
                            .bind(gateway_id)
                            .append(", ")
                            .bind(record.event_id)
                            .append(", ")
                            .bind(record.payload_sha256.as_str())
                            .append(", ")
                            .bind(payload)
                            .append(", ")
                            .bind(record.cursor.boot_epoch)
                            .append(", ")
                            .bind(i64::try_from(record.cursor.sequence).map_err(|_| {
                                RepositoryError::Storage(
                                    "inference usage sequence exceeds i64".into(),
                                )
                            })?)
                            .append(", ")
                            .bind(batch.batch_id)
                            .append(", ")
                            .bind(accepted_at)
                            .append(")"),
                        )
                        .await?;
                    }

                    let (boot_epoch, sequence) = match applied.next.watermark {
                        Some(cursor) => (
                            Some(cursor.boot_epoch),
                            Some(i64::try_from(cursor.sequence).map_err(|_| {
                                RepositoryError::Storage(
                                    "inference usage watermark sequence exceeds i64".into(),
                                )
                            })?),
                        ),
                        None => (None, None),
                    };
                    execute(
                        transaction,
                        sql_query::<()>("update inference_usage_watermarks set boot_epoch = ")
                            .bind(boot_epoch)
                            .append(", sequence = ")
                            .bind(sequence)
                            .append(", updated_at = ")
                            .bind(accepted_at)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and gateway_id = ")
                            .bind(gateway_id),
                    )
                    .await?;

                    persist_usage_projections(
                        transaction,
                        organization_id,
                        gateway_id,
                        &batch,
                        &applied.inserted_event_ids,
                        accepted_at,
                    )
                    .await?;

                    Ok(applied.receipt)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_daily_rollups(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
        from_day: NaiveDate,
        to_day: NaiveDate,
    ) -> Result<Vec<InferenceUsageDailyRollup>, RepositoryError> {
        if from_day > to_day {
            return Err(RepositoryError::Storage(
                "inference usage rollup from_day must be <= to_day".into(),
            ));
        }
        let organization_uuid = organization_id.as_uuid();
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<DailyRollupRow>(
                    "select day, environment_id, model_id, endpoint, request_count, succeeded_count, failed_count, fallback_count, cancelled_count, disconnected_count, unknown_measurement_count, upstream_usage_count, total_tokens from inference_usage_daily_rollups where organization_id = ",
                )
                .bind(organization_uuid)
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" and day >= ")
                .bind(from_day)
                .append(" and day <= ")
                .bind(to_day)
                .append(" order by day, environment_id, model_id, endpoint"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(|row| row.into_rollup())
            .collect()
    }

    async fn get_request_fact(
        &self,
        organization_id: OrganizationId,
        request_id: Uuid,
    ) -> Result<Option<InferenceUsageRequestFact>, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<RequestFactRow>(
                    "select request_id, gateway_id, environment_id, credential_id, credential_generation, route_id, route_policy_revision, endpoint, model_alias, model_id, started_at, terminated_at, outcome, http_status, duration_ms, measurement_completeness, total_tokens, attempt_count from inference_usage_request_facts where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and request_id = ")
                .bind(request_id),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(|row| row.into_fact()).transpose()
    }
}

async fn persist_usage_projections(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    gateway_id: Uuid,
    batch: &a3s_cloud_contracts::InferenceUsageBatchV1,
    inserted_event_ids: &[Uuid],
    accepted_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    if inserted_event_ids.is_empty() {
        return Ok(());
    }
    let inserted: HashSet<Uuid> = inserted_event_ids.iter().copied().collect();
    let mut request_ids = HashSet::new();
    for record in &batch.records {
        if !inserted.contains(&record.event_id) {
            continue;
        }
        let payload = record
            .payload()
            .map_err(|error| RepositoryError::Storage(error))?;
        let event = a3s_cloud_contracts::InferenceUsageLifecycleEventV1::decode(&payload)
            .map_err(RepositoryError::Storage)?;
        request_ids.insert(event.request.request_id);
    }

    let mut facts = HashMap::new();
    for request_id in &request_ids {
        let row: Option<RequestFactRow> = fetch_optional(
            transaction,
            sql_query::<RequestFactRow>(
                "select request_id, gateway_id, environment_id, credential_id, credential_generation, route_id, route_policy_revision, endpoint, model_alias, model_id, started_at, terminated_at, outcome, http_status, duration_ms, measurement_completeness, total_tokens, attempt_count from inference_usage_request_facts where organization_id = ",
            )
            .bind(organization_id)
            .append(" and request_id = ")
            .bind(*request_id),
        )
        .await?;
        if let Some(row) = row {
            let fact = row.into_fact()?;
            facts.insert(fact.request_id, fact);
        }
    }

    let mut rollups = HashMap::new();
    project_inserted_usage_records(
        &mut facts,
        &mut rollups,
        gateway_id,
        batch,
        inserted_event_ids,
    )
    .map_err(RepositoryError::Storage)?;

    for fact in facts.values() {
        if !request_ids.contains(&fact.request_id) {
            continue;
        }
        upsert_request_fact(transaction, organization_id, fact, accepted_at).await?;
    }
    for rollup in rollups.values() {
        upsert_daily_rollup_delta(transaction, organization_id, rollup, accepted_at).await?;
    }
    Ok(())
}

async fn upsert_request_fact(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    fact: &InferenceUsageRequestFact,
    accepted_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        sql_query::<()>(
            "insert into inference_usage_request_facts (organization_id, request_id, gateway_id, environment_id, credential_id, credential_generation, route_id, route_policy_revision, endpoint, model_alias, model_id, started_at, terminated_at, outcome, http_status, duration_ms, measurement_completeness, total_tokens, attempt_count, updated_at) values (",
        )
        .bind(organization_id)
        .append(", ")
        .bind(fact.request_id)
        .append(", ")
        .bind(fact.gateway_id)
        .append(", ")
        .bind(fact.environment_id)
        .append(", ")
        .bind(fact.credential_id)
        .append(", ")
        .bind(i64::try_from(fact.credential_generation).map_err(|_| {
            RepositoryError::Storage("credential_generation exceeds i64".into())
        })?)
        .append(", ")
        .bind(fact.route_id)
        .append(", ")
        .bind(i64::try_from(fact.route_policy_revision).map_err(|_| {
            RepositoryError::Storage("route_policy_revision exceeds i64".into())
        })?)
        .append(", ")
        .bind(endpoint_str(fact.endpoint))
        .append(", ")
        .bind(fact.model_alias.as_str())
        .append(", ")
        .bind(fact.model_id)
        .append(", ")
        .bind(fact.started_at)
        .append(", ")
        .bind(fact.terminated_at)
        .append(", ")
        .bind(outcome_str(fact.outcome))
        .append(", ")
        .bind(fact.http_status.map(i32::from))
        .append(", ")
        .bind(fact.duration_ms.map(|value| {
            i64::try_from(value).unwrap_or(i64::MAX)
        }))
        .append(", ")
        .bind(measurement_str(fact.measurement_completeness))
        .append(", ")
        .bind(fact.total_tokens.map(|value| {
            i64::try_from(value).unwrap_or(i64::MAX)
        }))
        .append(", ")
        .bind(i32::try_from(fact.attempt_count).unwrap_or(i32::MAX))
        .append(", ")
        .bind(accepted_at)
        .append(
            ") on conflict (organization_id, request_id) do update set gateway_id = excluded.gateway_id, environment_id = excluded.environment_id, credential_id = excluded.credential_id, credential_generation = excluded.credential_generation, route_id = excluded.route_id, route_policy_revision = excluded.route_policy_revision, endpoint = excluded.endpoint, model_alias = excluded.model_alias, model_id = excluded.model_id, started_at = excluded.started_at, terminated_at = excluded.terminated_at, outcome = excluded.outcome, http_status = excluded.http_status, duration_ms = excluded.duration_ms, measurement_completeness = excluded.measurement_completeness, total_tokens = excluded.total_tokens, attempt_count = excluded.attempt_count, updated_at = excluded.updated_at",
        ),
    )
    .await?;
    Ok(())
}

async fn upsert_daily_rollup_delta(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    rollup: &InferenceUsageDailyRollup,
    accepted_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        sql_query::<()>(
            "insert into inference_usage_daily_rollups (organization_id, day, environment_id, model_id, endpoint, request_count, succeeded_count, failed_count, fallback_count, cancelled_count, disconnected_count, unknown_measurement_count, upstream_usage_count, total_tokens, updated_at) values (",
        )
        .bind(organization_id)
        .append(", ")
        .bind(rollup.key.day)
        .append(", ")
        .bind(rollup.key.environment_id)
        .append(", ")
        .bind(rollup.key.model_id)
        .append(", ")
        .bind(endpoint_str(rollup.key.endpoint))
        .append(", ")
        .bind(i64::try_from(rollup.request_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.succeeded_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.failed_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.fallback_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.cancelled_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.disconnected_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.unknown_measurement_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.upstream_usage_count).unwrap_or(i64::MAX))
        .append(", ")
        .bind(i64::try_from(rollup.total_tokens).unwrap_or(i64::MAX))
        .append(", ")
        .bind(accepted_at)
        .append(
            ") on conflict (organization_id, day, environment_id, model_id, endpoint) do update set request_count = inference_usage_daily_rollups.request_count + excluded.request_count, succeeded_count = inference_usage_daily_rollups.succeeded_count + excluded.succeeded_count, failed_count = inference_usage_daily_rollups.failed_count + excluded.failed_count, fallback_count = inference_usage_daily_rollups.fallback_count + excluded.fallback_count, cancelled_count = inference_usage_daily_rollups.cancelled_count + excluded.cancelled_count, disconnected_count = inference_usage_daily_rollups.disconnected_count + excluded.disconnected_count, unknown_measurement_count = inference_usage_daily_rollups.unknown_measurement_count + excluded.unknown_measurement_count, upstream_usage_count = inference_usage_daily_rollups.upstream_usage_count + excluded.upstream_usage_count, total_tokens = inference_usage_daily_rollups.total_tokens + excluded.total_tokens, updated_at = excluded.updated_at",
        ),
    )
    .await?;
    Ok(())
}

fn endpoint_str(endpoint: InferenceUsageEndpointV1) -> &'static str {
    match endpoint {
        InferenceUsageEndpointV1::Models => "models",
        InferenceUsageEndpointV1::ChatCompletions => "chat-completions",
        InferenceUsageEndpointV1::Completions => "completions",
        InferenceUsageEndpointV1::Embeddings => "embeddings",
    }
}

fn parse_endpoint(value: &str) -> Result<InferenceUsageEndpointV1, RepositoryError> {
    match value {
        "models" => Ok(InferenceUsageEndpointV1::Models),
        "chat-completions" => Ok(InferenceUsageEndpointV1::ChatCompletions),
        "completions" => Ok(InferenceUsageEndpointV1::Completions),
        "embeddings" => Ok(InferenceUsageEndpointV1::Embeddings),
        other => Err(RepositoryError::Storage(format!(
            "unknown inference usage endpoint {other}"
        ))),
    }
}

fn outcome_str(outcome: Option<InferenceUsageTerminalOutcomeV1>) -> Option<&'static str> {
    outcome.map(|value| match value {
        InferenceUsageTerminalOutcomeV1::Succeeded => "succeeded",
        InferenceUsageTerminalOutcomeV1::Failed => "failed",
        InferenceUsageTerminalOutcomeV1::Fallback => "fallback",
        InferenceUsageTerminalOutcomeV1::Cancelled => "cancelled",
        InferenceUsageTerminalOutcomeV1::Disconnected => "disconnected",
    })
}

fn parse_outcome(value: Option<String>) -> Result<Option<InferenceUsageTerminalOutcomeV1>, RepositoryError> {
    match value.as_deref() {
        None => Ok(None),
        Some("succeeded") => Ok(Some(InferenceUsageTerminalOutcomeV1::Succeeded)),
        Some("failed") => Ok(Some(InferenceUsageTerminalOutcomeV1::Failed)),
        Some("fallback") => Ok(Some(InferenceUsageTerminalOutcomeV1::Fallback)),
        Some("cancelled") => Ok(Some(InferenceUsageTerminalOutcomeV1::Cancelled)),
        Some("disconnected") => Ok(Some(InferenceUsageTerminalOutcomeV1::Disconnected)),
        Some(other) => Err(RepositoryError::Storage(format!(
            "unknown inference usage outcome {other}"
        ))),
    }
}

fn measurement_str(
    value: Option<InferenceUsageMeasurementCompletenessV1>,
) -> Option<&'static str> {
    value.map(|value| match value {
        InferenceUsageMeasurementCompletenessV1::Unknown => "unknown",
        InferenceUsageMeasurementCompletenessV1::UpstreamUsage => "upstream_usage",
    })
}

fn parse_measurement(
    value: Option<String>,
) -> Result<Option<InferenceUsageMeasurementCompletenessV1>, RepositoryError> {
    match value.as_deref() {
        None => Ok(None),
        Some("unknown") => Ok(Some(InferenceUsageMeasurementCompletenessV1::Unknown)),
        Some("upstream_usage") => Ok(Some(InferenceUsageMeasurementCompletenessV1::UpstreamUsage)),
        Some(other) => Err(RepositoryError::Storage(format!(
            "unknown inference usage measurement {other}"
        ))),
    }
}

struct RequestFactRow {
    request_id: Uuid,
    gateway_id: Uuid,
    environment_id: Uuid,
    credential_id: Uuid,
    credential_generation: i64,
    route_id: Uuid,
    route_policy_revision: i64,
    endpoint: String,
    model_alias: String,
    model_id: Uuid,
    started_at: DateTime<Utc>,
    terminated_at: Option<DateTime<Utc>>,
    outcome: Option<String>,
    http_status: Option<i32>,
    duration_ms: Option<i64>,
    measurement_completeness: Option<String>,
    total_tokens: Option<i64>,
    attempt_count: i32,
}

impl FromRow for RequestFactRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            request_id: decode(row, 0)?,
            gateway_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            credential_id: decode(row, 3)?,
            credential_generation: decode(row, 4)?,
            route_id: decode(row, 5)?,
            route_policy_revision: decode(row, 6)?,
            endpoint: decode(row, 7)?,
            model_alias: decode(row, 8)?,
            model_id: decode(row, 9)?,
            started_at: decode(row, 10)?,
            terminated_at: decode(row, 11)?,
            outcome: decode(row, 12)?,
            http_status: decode(row, 13)?,
            duration_ms: decode(row, 14)?,
            measurement_completeness: decode(row, 15)?,
            total_tokens: decode(row, 16)?,
            attempt_count: decode(row, 17)?,
        })
    }
}

impl RequestFactRow {
    fn into_fact(self) -> Result<InferenceUsageRequestFact, RepositoryError> {
        Ok(InferenceUsageRequestFact {
            request_id: self.request_id,
            gateway_id: self.gateway_id,
            environment_id: self.environment_id,
            credential_id: self.credential_id,
            credential_generation: u64::try_from(self.credential_generation).map_err(|_| {
                RepositoryError::Storage("credential_generation is negative".into())
            })?,
            route_id: self.route_id,
            route_policy_revision: u64::try_from(self.route_policy_revision).map_err(|_| {
                RepositoryError::Storage("route_policy_revision is negative".into())
            })?,
            endpoint: parse_endpoint(&self.endpoint)?,
            model_alias: self.model_alias,
            model_id: self.model_id,
            started_at: self.started_at,
            terminated_at: self.terminated_at,
            outcome: parse_outcome(self.outcome)?,
            http_status: self
                .http_status
                .map(|value| u16::try_from(value).map_err(|_| {
                    RepositoryError::Storage("http_status out of range".into())
                }))
                .transpose()?,
            duration_ms: self
                .duration_ms
                .map(|value| u64::try_from(value).map_err(|_| {
                    RepositoryError::Storage("duration_ms is negative".into())
                }))
                .transpose()?,
            measurement_completeness: parse_measurement(self.measurement_completeness)?,
            total_tokens: self
                .total_tokens
                .map(|value| u64::try_from(value).map_err(|_| {
                    RepositoryError::Storage("total_tokens is negative".into())
                }))
                .transpose()?,
            attempt_count: u32::try_from(self.attempt_count).map_err(|_| {
                RepositoryError::Storage("attempt_count is negative".into())
            })?,
        })
    }
}

struct DailyRollupRow {
    day: NaiveDate,
    environment_id: Uuid,
    model_id: Uuid,
    endpoint: String,
    request_count: i64,
    succeeded_count: i64,
    failed_count: i64,
    fallback_count: i64,
    cancelled_count: i64,
    disconnected_count: i64,
    unknown_measurement_count: i64,
    upstream_usage_count: i64,
    total_tokens: i64,
}

impl FromRow for DailyRollupRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            day: decode(row, 0)?,
            environment_id: decode(row, 1)?,
            model_id: decode(row, 2)?,
            endpoint: decode(row, 3)?,
            request_count: decode(row, 4)?,
            succeeded_count: decode(row, 5)?,
            failed_count: decode(row, 6)?,
            fallback_count: decode(row, 7)?,
            cancelled_count: decode(row, 8)?,
            disconnected_count: decode(row, 9)?,
            unknown_measurement_count: decode(row, 10)?,
            upstream_usage_count: decode(row, 11)?,
            total_tokens: decode(row, 12)?,
        })
    }
}

impl DailyRollupRow {
    fn into_rollup(self) -> Result<InferenceUsageDailyRollup, RepositoryError> {
        Ok(InferenceUsageDailyRollup {
            key: InferenceUsageDailyRollupKey {
                day: self.day,
                environment_id: self.environment_id,
                model_id: self.model_id,
                endpoint: parse_endpoint(&self.endpoint)?,
            },
            request_count: u64::try_from(self.request_count).map_err(|_| {
                RepositoryError::Storage("request_count is negative".into())
            })?,
            succeeded_count: u64::try_from(self.succeeded_count).map_err(|_| {
                RepositoryError::Storage("succeeded_count is negative".into())
            })?,
            failed_count: u64::try_from(self.failed_count).map_err(|_| {
                RepositoryError::Storage("failed_count is negative".into())
            })?,
            fallback_count: u64::try_from(self.fallback_count).map_err(|_| {
                RepositoryError::Storage("fallback_count is negative".into())
            })?,
            cancelled_count: u64::try_from(self.cancelled_count).map_err(|_| {
                RepositoryError::Storage("cancelled_count is negative".into())
            })?,
            disconnected_count: u64::try_from(self.disconnected_count).map_err(|_| {
                RepositoryError::Storage("disconnected_count is negative".into())
            })?,
            unknown_measurement_count: u64::try_from(self.unknown_measurement_count).map_err(
                |_| RepositoryError::Storage("unknown_measurement_count is negative".into()),
            )?,
            upstream_usage_count: u64::try_from(self.upstream_usage_count).map_err(|_| {
                RepositoryError::Storage("upstream_usage_count is negative".into())
            })?,
            total_tokens: u64::try_from(self.total_tokens).map_err(|_| {
                RepositoryError::Storage("total_tokens is negative".into())
            })?,
        })
    }
}

#[cfg(test)]
mod typed_surface_tests {
    #[test]
    fn postgres_inference_usage_persistence_stays_on_a3s_orm() {
        let source = include_str!("postgres.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production surface precedes cfg(test)");
        for forbidden in ["sqlx::", "tokio_postgres", "diesel::"] {
            assert!(
                !production.contains(forbidden),
                "inference usage postgres adapter introduced {forbidden}"
            );
        }
        assert!(production.contains("sql_query"));
        assert!(production.contains("PostgresExecutor"));
    }
}
