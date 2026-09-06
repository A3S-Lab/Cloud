use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, store_audit, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::automations::domain::{
    AutomationInvocationAdmission, AutomationInvocationRecord, IAutomationInvocationReader,
    IAutomationInvocationRepository,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId, RepositoryError};
use a3s_cloud_contracts::{
    AutomationAuditActionV1, AutomationAuditRecordV1, AutomationInvocationEnvelopeV1,
    AutomationOutboxMessageV1, CloudScopeRef, DomainEventEnvelope,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

const SELECT_INVOCATION: &str = "select organization_id, project_id, environment_id, automation_id, invocation_id, revision_id, revision_digest, deduplication_key, invocation_digest, invocation_json, requested_at, admitted_at from automation_invocations";

/// Durable PostgreSQL implementation of the single Automations admission port.
///
/// The transaction locks the exact invocation identity and policy key before
/// deciding between first admission, exact replay, and immutable-evidence
/// conflict. Audit and Outbox facts commit with the invocation row.
#[derive(Clone)]
pub struct PostgresAutomationInvocationRepository {
    executor: PostgresExecutor,
}

impl PostgresAutomationInvocationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAutomationInvocationRepository for PostgresAutomationInvocationRepository {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> Result<AutomationInvocationAdmission, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    admit_invocation_in_transaction(transaction, envelope, true).await
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IAutomationInvocationReader for PostgresAutomationInvocationRepository {
    async fn find(
        &self,
        organization_id: Uuid,
        invocation_id: Uuid,
    ) -> Result<Option<AutomationInvocationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<InvocationRow, _>(
                        transaction,
                        sql_query::<InvocationRow>(SELECT_INVOCATION)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and invocation_id = ")
                            .bind(invocation_id),
                    )
                    .await?
                    .map(decode_invocation)
                    .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

pub(super) async fn admit_invocation_in_transaction(
    transaction: &PostgresTransaction,
    envelope: AutomationInvocationEnvelopeV1,
    emit_outbox: bool,
) -> Result<AutomationInvocationAdmission, PostgresPersistenceError> {
    let record = AutomationInvocationRecord::new(envelope)
        .map_err(|error| PostgresPersistenceError::Repository(RepositoryError::Conflict(error)))?;
    let existing = fetch_optional::<InvocationRow, _>(
        transaction,
        sql_query::<InvocationRow>(SELECT_INVOCATION)
            .append(" where organization_id = ")
            .bind(record.envelope.organization_id)
            .append(" and invocation_id = ")
            .bind(record.envelope.invocation_id)
            .append(" for update"),
    )
    .await?
    .map(decode_invocation)
    .transpose()?;

    if let Some(existing) = existing {
        if existing == record {
            persist_side_effects(transaction, &record, true, emit_outbox).await?;
            return Ok(AutomationInvocationAdmission {
                invocation: existing,
                replayed: true,
            });
        }
        return Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "Automation invocation identity was reused with different immutable evidence"
                    .into(),
            ),
        ));
    }

    let duplicate = fetch_optional::<InvocationRow, _>(
        transaction,
        sql_query::<InvocationRow>(SELECT_INVOCATION)
            .append(" where organization_id = ")
            .bind(record.envelope.organization_id)
            .append(" and automation_id = ")
            .bind(record.envelope.automation_id)
            .append(" and deduplication_key = ")
            .bind(record.envelope.deduplication_key.as_str())
            .append(" for update"),
    )
    .await?
    .map(decode_invocation)
    .transpose()?;
    if let Some(existing) = duplicate {
        return Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(format!(
                "Automation deduplication key is already bound to invocation {}",
                existing.envelope.invocation_id
            )),
        ));
    }

    let inserted = insert_invocation(transaction, &record).await?;
    if !inserted {
        // `ON CONFLICT DO NOTHING` keeps the transaction usable after a
        // concurrent winner. Re-read both identity indexes and apply the same
        // exact-replay versus deduplication-drift rules as the fast path.
        let concurrent = fetch_optional::<InvocationRow, _>(
            transaction,
            sql_query::<InvocationRow>(SELECT_INVOCATION)
                .append(" where organization_id = ")
                .bind(record.envelope.organization_id)
                .append(" and invocation_id = ")
                .bind(record.envelope.invocation_id)
                .append(" for update"),
        )
        .await?
        .map(decode_invocation)
        .transpose()?;
        if let Some(existing) = concurrent {
            if existing == record {
                persist_side_effects(transaction, &record, true, emit_outbox).await?;
                return Ok(AutomationInvocationAdmission {
                    invocation: existing,
                    replayed: true,
                });
            }
            return Err(PostgresPersistenceError::Repository(
                RepositoryError::Conflict(
                    "Automation invocation identity was reused with different immutable evidence"
                        .into(),
                ),
            ));
        }
        return Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "Automation deduplication key is already bound to another invocation".into(),
            ),
        ));
    }
    persist_side_effects(transaction, &record, false, emit_outbox).await?;
    Ok(AutomationInvocationAdmission {
        invocation: record,
        replayed: false,
    })
}

async fn insert_invocation(
    transaction: &PostgresTransaction,
    record: &AutomationInvocationRecord,
) -> Result<bool, PostgresPersistenceError> {
    let envelope = &record.envelope;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_invocations (organization_id, project_id, environment_id, automation_id, invocation_id, revision_id, revision_digest, deduplication_key, invocation_digest, invocation_json, requested_at, admitted_at) values (")
            .bind(envelope.organization_id)
            .append(", ")
            .bind(envelope.project_id)
            .append(", ")
            .bind(envelope.environment_id)
            .append(", ")
            .bind(envelope.automation_id)
            .append(", ")
            .bind(envelope.invocation_id)
            .append(", ")
            .bind(envelope.automation_revision_id)
            .append(", ")
            .bind(envelope.automation_revision_digest.as_str())
            .append(", ")
            .bind(envelope.deduplication_key.as_str())
            .append(", ")
            .bind(record.digest.as_str())
            .append(", ")
            .bind(serde_json::to_value(envelope)?)
            .append(", ")
            .bind(envelope.requested_at)
            .append(", ")
            .bind(envelope.requested_at)
            .append(") on conflict do nothing"),
    )
    .await;
    match rows {
        Ok(rows) => Ok(rows == 1),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn persist_side_effects(
    transaction: &PostgresTransaction,
    record: &AutomationInvocationRecord,
    replayed: bool,
    emit_outbox: bool,
) -> Result<(), PostgresPersistenceError> {
    let envelope = &record.envelope;
    let action = if replayed {
        AutomationAuditActionV1::InvocationReplayed
    } else {
        AutomationAuditActionV1::InvocationAdmitted
    };
    let audit = AutomationAuditRecordV1::for_invocation(
        envelope,
        action,
        Uuid::now_v7(),
        envelope.requested_at,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: audit.audit_id,
            scope: AuditWrite::resource_scope(
                audit.organization_id,
                ProjectId::from_uuid(audit.project_id),
                Some(EnvironmentId::from_uuid(audit.environment_id)),
            ),
            actor_id: audit.actor_id,
            action: if replayed {
                "automation.invocation.replayed"
            } else {
                "automation.invocation.admitted"
            },
            aggregate_id: audit.automation_id,
            occurred_at: audit.occurred_at,
            request_id: audit.correlation_id,
            details: serde_json::json!({
                "schema": audit.schema,
                "automationId": audit.automation_id,
                "revisionId": audit.revision_id,
                "invocationId": envelope.invocation_id,
                "invocationDigest": record.digest,
                "deduplicationKey": envelope.deduplication_key,
                "replayed": replayed,
            }),
        },
    )
    .await?;

    if !replayed && emit_outbox {
        let outbox = AutomationOutboxMessageV1::for_invocation(
            envelope,
            Uuid::now_v7(),
            envelope.causation_id,
            envelope.requested_at,
        )
        .map_err(PostgresPersistenceError::Invariant)?;
        store_outbox(transaction, &outbox_event(&outbox)?).await?;
    }
    Ok(())
}

fn outbox_event(
    message: &AutomationOutboxMessageV1,
) -> Result<DomainEventEnvelope, PostgresPersistenceError> {
    Ok(DomainEventEnvelope {
        event_id: message.message_id,
        event_key: message.event_key().into(),
        schema_version: message.event_version,
        scope: CloudScopeRef::Environment {
            organization_id: message.organization_id,
            project_id: message.project_id,
            environment_id: message.environment_id,
        },
        aggregate_id: message.automation_id,
        aggregate_version: 1,
        occurred_at: message.occurred_at,
        correlation_id: message.correlation_id,
        causation_id: message.causation_id,
        payload: serde_json::to_value(message)?,
    })
}

struct InvocationRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    automation_id: Uuid,
    invocation_id: Uuid,
    revision_id: Uuid,
    revision_digest: String,
    deduplication_key: String,
    invocation_digest: String,
    invocation_json: Value,
    requested_at: chrono::DateTime<chrono::Utc>,
    admitted_at: chrono::DateTime<chrono::Utc>,
}

impl FromRow for InvocationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            automation_id: decode(row, 3)?,
            invocation_id: decode(row, 4)?,
            revision_id: decode(row, 5)?,
            revision_digest: decode(row, 6)?,
            deduplication_key: decode(row, 7)?,
            invocation_digest: decode(row, 8)?,
            invocation_json: decode(row, 9)?,
            requested_at: decode(row, 10)?,
            admitted_at: decode(row, 11)?,
        })
    }
}

fn decode_invocation(
    row: InvocationRow,
) -> Result<AutomationInvocationRecord, PostgresPersistenceError> {
    let envelope: AutomationInvocationEnvelopeV1 = serde_json::from_value(row.invocation_json)
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored Automation invocation is invalid: {error}"
            ))
        })?;
    if envelope.organization_id != row.organization_id
        || envelope.project_id != row.project_id
        || envelope.environment_id != row.environment_id
        || envelope.automation_id != row.automation_id
        || envelope.invocation_id != row.invocation_id
        || envelope.automation_revision_id != row.revision_id
        || envelope.automation_revision_digest != row.revision_digest
        || envelope.deduplication_key != row.deduplication_key
        || envelope.requested_at != row.requested_at
        || row.admitted_at < row.requested_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored Automation invocation projection drifted".into(),
        ));
    }
    let record =
        AutomationInvocationRecord::new(envelope).map_err(PostgresPersistenceError::Invariant)?;
    if record.digest != row.invocation_digest {
        return Err(PostgresPersistenceError::Invariant(
            "stored Automation invocation digest drifted".into(),
        ));
    }
    Ok(record)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
