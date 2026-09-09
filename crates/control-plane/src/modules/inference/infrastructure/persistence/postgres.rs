use crate::infrastructure::{
    execute, fetch_all, fetch_optional, transaction_error, PostgresPersistenceError,
};
use crate::modules::inference::domain::{
    apply_inference_usage_batch, AcceptInferenceUsageBatchWrite, IInferenceUsageRepository,
    InferenceUsageLedgerError, InferenceUsageLedgerState,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{InferenceUsageCursorV1, InferenceUsageReceiptV1};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use std::collections::HashMap;
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
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into inference_usage_events (organization_id, gateway_id, event_id, payload_sha256, boot_epoch, sequence, batch_id, accepted_at) values (",
                            )
                            .bind(organization_id)
                            .append(", ")
                            .bind(gateway_id)
                            .append(", ")
                            .bind(record.event_id)
                            .append(", ")
                            .bind(record.payload_sha256.as_str())
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

                    Ok(applied.receipt)
                })
            })
            .await
            .map_err(transaction_error)
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
