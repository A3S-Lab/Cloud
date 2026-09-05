use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_audit, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::automations::domain::{
    AdmitAutomationWebhookDeliveryWrite, AutomationWebhookAdmission,
    AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord, EndpointLifecycleAction,
    IAutomationWebhookRepository, TransitionAutomationWebhookEndpoint,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId, RepositoryError};
use a3s_cloud_contracts::{
    AutomationAuditActionV1, AutomationAuditRecordV1, AutomationOutboxMessageV1,
    AutomationWebhookAdmissionDecisionV1, AutomationWebhookDeliveryReceiptV1,
    AutomationWebhookEndpointStateV1, AutomationWebhookRejectionReasonV1,
    AutomationWebhookRequestV1, CloudScopeRef, DomainEventEnvelope,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

const SELECT_ENDPOINT: &str = "select organization_id, project_id, environment_id, endpoint_id, endpoint_key, revision_digest, revision_acl, endpoint_json from automation_webhook_endpoints";
const SELECT_DELIVERY: &str = "select organization_id, endpoint_id, delivery_id, request_json, receipt_json, invocation_json, body_digest, decision, first_received_at, recorded_at from automation_webhook_deliveries";

#[derive(Clone)]
pub struct PostgresAutomationWebhookRepository {
    executor: PostgresExecutor,
}

impl PostgresAutomationWebhookRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAutomationWebhookRepository for PostgresAutomationWebhookRepository {
    async fn create_endpoint(
        &self,
        record: AutomationWebhookEndpointRecord,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    record
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_endpoint(transaction, &record).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_endpoint(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<AutomationWebhookEndpointRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { load_endpoint(transaction, endpoint_id, false).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn transition_endpoint(
        &self,
        transition: TransitionAutomationWebhookEndpoint,
    ) -> Result<AutomationWebhookEndpointRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = load_endpoint(transaction, transition.endpoint_id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if current.endpoint.generation != transition.expected_generation {
                        return Err(RepositoryError::Conflict(
                            "Automation webhook endpoint generation is stale".into(),
                        )
                        .into());
                    }
                    let mut endpoint = current.endpoint.clone();
                    match transition.action {
                        EndpointLifecycleAction::Disable => endpoint
                            .disable(transition.changed_at)
                            .map_err(RepositoryError::Conflict)?,
                        EndpointLifecycleAction::Enable => endpoint
                            .enable(transition.changed_at)
                            .map_err(RepositoryError::Conflict)?,
                        EndpointLifecycleAction::Revoke => endpoint
                            .revoke(transition.changed_at)
                            .map_err(RepositoryError::Conflict)?,
                    }
                    let updated = AutomationWebhookEndpointRecord::new(endpoint, current.revision)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update automation_webhook_endpoints set endpoint_json = ")
                            .bind(serde_json::to_value(&updated.endpoint)?)
                            .append(", generation = ")
                            .bind(updated.endpoint.generation)
                            .append(", state = ")
                            .bind(endpoint_state(updated.endpoint.state))
                            .append(", state_changed_at = ")
                            .bind(updated.endpoint.state_changed_at)
                            .append(" where organization_id = ")
                            .bind(updated.endpoint.organization_id)
                            .append(" and endpoint_id = ")
                            .bind(updated.endpoint.endpoint_id)
                            .append(" and generation = ")
                            .bind(transition.expected_generation),
                    )
                    .await?;
                    match rows {
                        1 => Ok(updated),
                        0 => Err(RepositoryError::Conflict(
                            "Automation webhook endpoint generation is stale".into(),
                        )
                        .into()),
                        rows => Err(PostgresPersistenceError::Invariant(format!(
                            "Automation webhook endpoint transition affected {rows} rows"
                        ))),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_delivery(
        &self,
        endpoint_id: Uuid,
        delivery_id: Uuid,
    ) -> Result<Option<AutomationWebhookDeliveryRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(endpoint) = load_endpoint(transaction, endpoint_id, false).await?
                    else {
                        return Ok(None);
                    };
                    let Some(row) = fetch_optional::<DeliveryRow, _>(
                        transaction,
                        sql_query::<DeliveryRow>(SELECT_DELIVERY)
                            .append(" where organization_id = ")
                            .bind(endpoint.endpoint.organization_id)
                            .append(" and endpoint_id = ")
                            .bind(endpoint_id)
                            .append(" and delivery_id = ")
                            .bind(delivery_id),
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    decode_delivery(row, &endpoint).map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn admit_delivery(
        &self,
        write: AdmitAutomationWebhookDeliveryWrite,
    ) -> Result<AutomationWebhookAdmission, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { admit_delivery(transaction, write).await })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_endpoint(
    transaction: &PostgresTransaction,
    record: &AutomationWebhookEndpointRecord,
) -> Result<AutomationWebhookEndpointRecord, PostgresPersistenceError> {
    let endpoint = &record.endpoint;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_webhook_endpoints (organization_id, project_id, environment_id, endpoint_id, endpoint_key, revision_id, revision_digest, revision_acl, endpoint_json, generation, state, created_at, state_changed_at) values (")
            .bind(endpoint.organization_id)
            .append(", ")
            .bind(endpoint.project_id)
            .append(", ")
            .bind(endpoint.environment_id)
            .append(", ")
            .bind(endpoint.endpoint_id)
            .append(", ")
            .bind(endpoint.endpoint_key.as_str())
            .append(", ")
            .bind(endpoint.revision_id)
            .append(", ")
            .bind(endpoint.revision_digest.as_str())
            .append(", ")
            .bind(record.revision.canonical_acl())
            .append(", ")
            .bind(serde_json::to_value(endpoint)?)
            .append(", ")
            .bind(endpoint.generation)
            .append(", ")
            .bind(endpoint_state(endpoint.state))
            .append(", ")
            .bind(endpoint.created_at)
            .append(", ")
            .bind(endpoint.state_changed_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => {
            require_one_row("Automation webhook endpoint", rows)?;
            Ok(record.clone())
        }
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "Automation webhook endpoint identity or key is already in use".into(),
            ),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn load_endpoint(
    transaction: &PostgresTransaction,
    endpoint_id: Uuid,
    for_update: bool,
) -> Result<Option<AutomationWebhookEndpointRecord>, PostgresPersistenceError> {
    let query = sql_query::<EndpointRow>(SELECT_ENDPOINT)
        .append(" where endpoint_id = ")
        .bind(endpoint_id);
    let query = if for_update {
        query.append(" for update")
    } else {
        query
    };
    fetch_optional(transaction, query)
        .await?
        .map(decode_endpoint)
        .transpose()
}

async fn admit_delivery(
    transaction: &PostgresTransaction,
    write: AdmitAutomationWebhookDeliveryWrite,
) -> Result<AutomationWebhookAdmission, PostgresPersistenceError> {
    let endpoint_id = write.request.endpoint_id;
    let endpoint = load_endpoint(transaction, endpoint_id, true)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    endpoint
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    write
        .request
        .validate_for_endpoint(&endpoint.endpoint)
        .map_err(PostgresPersistenceError::Invariant)?;

    let existing = fetch_optional::<DeliveryRow, _>(
        transaction,
        sql_query::<DeliveryRow>(SELECT_DELIVERY)
            .append(" where organization_id = ")
            .bind(endpoint.endpoint.organization_id)
            .append(" and endpoint_id = ")
            .bind(endpoint_id)
            .append(" and delivery_id = ")
            .bind(write.request.delivery_id)
            .append(" for update"),
    )
    .await?
    .map(|row| decode_delivery(row, &endpoint))
    .transpose()?;

    if let Some(existing) = existing {
        let (receipt, invocation, replayed) = if existing.receipt.decision
            != AutomationWebhookAdmissionDecisionV1::Rejected
            && existing.request.body_digest == write.request.body_digest
        {
            (
                AutomationWebhookDeliveryReceiptV1::replay_of(
                    write.receipt_id,
                    &existing.receipt,
                    &endpoint.endpoint,
                    &endpoint.revision,
                    &write.request,
                    write.recorded_at,
                )
                .map_err(PostgresPersistenceError::Invariant)?,
                existing.invocation.clone(),
                true,
            )
        } else {
            (
                AutomationWebhookDeliveryReceiptV1::rejected(
                    write.receipt_id,
                    &endpoint.endpoint,
                    &endpoint.revision,
                    &write.request,
                    AutomationWebhookRejectionReasonV1::DuplicateDeliveryConflict,
                    write.recorded_at,
                )
                .map_err(PostgresPersistenceError::Invariant)?,
                None,
                false,
            )
        };
        let delivery = AutomationWebhookDeliveryRecord::new(
            write.request,
            receipt,
            invocation,
            &endpoint.endpoint,
            &endpoint.revision,
        )
        .map_err(PostgresPersistenceError::Invariant)?;
        insert_receipt(transaction, &delivery, endpoint.endpoint.organization_id).await?;
        persist_side_effects(transaction, &delivery, replayed).await?;
        return Ok(AutomationWebhookAdmission { delivery, replayed });
    }

    let (receipt, invocation) = if !endpoint.endpoint.state.is_accepting() {
        let reason = match endpoint.endpoint.state {
            AutomationWebhookEndpointStateV1::Disabled => {
                AutomationWebhookRejectionReasonV1::EndpointDisabled
            }
            AutomationWebhookEndpointStateV1::Revoked => {
                AutomationWebhookRejectionReasonV1::EndpointRevoked
            }
            AutomationWebhookEndpointStateV1::Active => {
                return Err(PostgresPersistenceError::Invariant(
                    "inactive webhook endpoint state changed during admission".into(),
                ))
            }
        };
        (
            AutomationWebhookDeliveryReceiptV1::rejected(
                write.receipt_id,
                &endpoint.endpoint,
                &endpoint.revision,
                &write.request,
                reason,
                write.recorded_at,
            )
            .map_err(PostgresPersistenceError::Invariant)?,
            None,
        )
    } else {
        let invocation = write.invocation.ok_or_else(|| {
            PostgresPersistenceError::Repository(RepositoryError::Conflict(
                "an active Automation webhook delivery requires an invocation envelope".into(),
            ))
        })?;
        let receipt = AutomationWebhookDeliveryReceiptV1::admitted(
            write.receipt_id,
            &endpoint.endpoint,
            &endpoint.revision,
            &write.request,
            &invocation,
            write.recorded_at,
        )
        .map_err(PostgresPersistenceError::Invariant)?;
        (receipt, Some(invocation))
    };
    let delivery = AutomationWebhookDeliveryRecord::new(
        write.request,
        receipt,
        invocation,
        &endpoint.endpoint,
        &endpoint.revision,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    insert_delivery(transaction, &delivery, endpoint.endpoint.organization_id).await?;
    insert_receipt(transaction, &delivery, endpoint.endpoint.organization_id).await?;
    persist_side_effects(transaction, &delivery, false).await?;
    Ok(AutomationWebhookAdmission {
        delivery,
        replayed: false,
    })
}

async fn insert_delivery(
    transaction: &PostgresTransaction,
    delivery: &AutomationWebhookDeliveryRecord,
    organization_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let endpoint = delivery.request.endpoint_id;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_webhook_deliveries (organization_id, endpoint_id, delivery_id, request_json, receipt_json, invocation_json, body_digest, decision, first_received_at, recorded_at) values (")
            .bind(organization_id)
            .append(", ")
            .bind(endpoint)
            .append(", ")
            .bind(delivery.request.delivery_id)
            .append(", ")
            .bind(serde_json::to_value(&delivery.request)?)
            .append(", ")
            .bind(serde_json::to_value(&delivery.receipt)?)
            .append(", ")
            .bind(delivery.invocation.as_ref().map(serde_json::to_value).transpose()?)
            .append(", ")
            .bind(delivery.request.body_digest.as_str())
            .append(", ")
            .bind(canonical_delivery_decision(delivery.receipt.decision)?)
            .append(", ")
            .bind(delivery.request.received_at)
            .append(", ")
            .bind(delivery.receipt.recorded_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("Automation webhook delivery", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "Automation webhook delivery identity is already in use".into(),
            ),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_receipt(
    transaction: &PostgresTransaction,
    delivery: &AutomationWebhookDeliveryRecord,
    organization_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_webhook_delivery_receipts (receipt_id, organization_id, endpoint_id, delivery_id, decision, receipt_json, recorded_at) values (")
            .bind(delivery.receipt.receipt_id)
            .append(", ")
            .bind(organization_id)
            .append(", ")
            .bind(delivery.receipt.endpoint_id)
            .append(", ")
            .bind(delivery.receipt.delivery_id)
            .append(", ")
            .bind(receipt_decision(delivery.receipt.decision))
            .append(", ")
            .bind(serde_json::to_value(&delivery.receipt)?)
            .append(", ")
            .bind(delivery.receipt.recorded_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("Automation webhook delivery receipt", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "Automation webhook receipt identity is already in use".into(),
            ),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn persist_side_effects(
    transaction: &PostgresTransaction,
    delivery: &AutomationWebhookDeliveryRecord,
    replayed: bool,
) -> Result<(), PostgresPersistenceError> {
    let Some(invocation) = &delivery.invocation else {
        return Ok(());
    };
    let action = if replayed {
        AutomationAuditActionV1::InvocationReplayed
    } else {
        AutomationAuditActionV1::InvocationAdmitted
    };
    let audit = AutomationAuditRecordV1::for_invocation(
        invocation,
        action,
        Uuid::now_v7(),
        delivery.receipt.recorded_at,
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
                "automation.webhook.invocation.replayed"
            } else {
                "automation.webhook.invocation.admitted"
            },
            aggregate_id: audit.automation_id,
            occurred_at: audit.occurred_at,
            request_id: audit.correlation_id,
            details: serde_json::json!({
                "schema": audit.schema,
                "endpointId": delivery.receipt.endpoint_id,
                "deliveryId": delivery.receipt.delivery_id,
                "automationId": audit.automation_id,
                "revisionId": audit.revision_id,
                "invocationId": invocation.invocation_id,
                "bodyDigest": delivery.receipt.body_digest,
                "decision": receipt_decision(delivery.receipt.decision),
            }),
        },
    )
    .await?;

    if !replayed && delivery.receipt.decision == AutomationWebhookAdmissionDecisionV1::Admitted {
        let outbox = AutomationOutboxMessageV1::for_invocation(
            invocation,
            Uuid::now_v7(),
            Some(delivery.request.delivery_id),
            delivery.receipt.recorded_at,
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

struct EndpointRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key: String,
    revision_digest: String,
    revision_acl: String,
    endpoint_json: Value,
}

impl FromRow for EndpointRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            endpoint_id: decode(row, 3)?,
            endpoint_key: decode(row, 4)?,
            revision_digest: decode(row, 5)?,
            revision_acl: decode(row, 6)?,
            endpoint_json: decode(row, 7)?,
        })
    }
}

struct DeliveryRow {
    organization_id: Uuid,
    endpoint_id: Uuid,
    delivery_id: Uuid,
    request_json: Value,
    receipt_json: Value,
    invocation_json: Option<Value>,
    body_digest: String,
    decision: String,
    first_received_at: chrono::DateTime<chrono::Utc>,
    recorded_at: chrono::DateTime<chrono::Utc>,
}

impl FromRow for DeliveryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            endpoint_id: decode(row, 1)?,
            delivery_id: decode(row, 2)?,
            request_json: decode(row, 3)?,
            receipt_json: decode(row, 4)?,
            invocation_json: decode(row, 5)?,
            body_digest: decode(row, 6)?,
            decision: decode(row, 7)?,
            first_received_at: decode(row, 8)?,
            recorded_at: decode(row, 9)?,
        })
    }
}

fn decode_endpoint(
    row: EndpointRow,
) -> Result<AutomationWebhookEndpointRecord, PostgresPersistenceError> {
    let revision =
        a3s_cloud_contracts::AutomationRevisionV1::restore(&row.revision_acl, &row.revision_digest)
            .map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored Automation revision is invalid: {error}"
                ))
            })?;
    let endpoint: a3s_cloud_contracts::AutomationWebhookEndpointV1 =
        serde_json::from_value(row.endpoint_json).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored Automation webhook endpoint is invalid: {error}"
            ))
        })?;
    if endpoint.organization_id != row.organization_id
        || endpoint.project_id != row.project_id
        || endpoint.environment_id != row.environment_id
        || endpoint.endpoint_id != row.endpoint_id
        || endpoint.endpoint_key != row.endpoint_key
        || endpoint.revision_digest != row.revision_digest
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored Automation webhook endpoint scope or revision drifted".into(),
        ));
    }
    AutomationWebhookEndpointRecord::new(endpoint, revision)
        .map_err(PostgresPersistenceError::Invariant)
}

fn decode_delivery(
    row: DeliveryRow,
    endpoint: &AutomationWebhookEndpointRecord,
) -> Result<AutomationWebhookDeliveryRecord, PostgresPersistenceError> {
    if row.organization_id != endpoint.endpoint.organization_id
        || row.endpoint_id != endpoint.endpoint.endpoint_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored Automation webhook delivery scope drifted".into(),
        ));
    }
    let request: AutomationWebhookRequestV1 =
        serde_json::from_value(row.request_json).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored webhook request is invalid: {error}"
            ))
        })?;
    let receipt: AutomationWebhookDeliveryReceiptV1 = serde_json::from_value(row.receipt_json)
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored webhook receipt is invalid: {error}"
            ))
        })?;
    let invocation = row
        .invocation_json
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored webhook invocation is invalid: {error}"
            ))
        })?;
    if request.delivery_id != row.delivery_id
        || request.endpoint_id != endpoint.endpoint.endpoint_id
        || request.body_digest != row.body_digest
        || receipt.endpoint_id != endpoint.endpoint.endpoint_id
        || receipt.delivery_id != row.delivery_id
        || receipt.automation_id != endpoint.endpoint.automation_id
        || receipt.revision_id != endpoint.endpoint.revision_id
        || receipt.revision_digest != endpoint.endpoint.revision_digest
        || receipt.body_digest != request.body_digest
        || receipt.first_received_at != request.received_at
        || row.decision != canonical_delivery_decision(receipt.decision)?
        || request.received_at != row.first_received_at
        || receipt.recorded_at != row.recorded_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored Automation webhook delivery projection drifted".into(),
        ));
    }
    AutomationWebhookDeliveryRecord::new(
        request,
        receipt,
        invocation,
        &endpoint.endpoint,
        &endpoint.revision,
    )
    .map_err(PostgresPersistenceError::Invariant)
}

fn endpoint_state(state: AutomationWebhookEndpointStateV1) -> &'static str {
    match state {
        AutomationWebhookEndpointStateV1::Active => "active",
        AutomationWebhookEndpointStateV1::Disabled => "disabled",
        AutomationWebhookEndpointStateV1::Revoked => "revoked",
    }
}

fn receipt_decision(decision: AutomationWebhookAdmissionDecisionV1) -> &'static str {
    match decision {
        AutomationWebhookAdmissionDecisionV1::Admitted => "admitted",
        AutomationWebhookAdmissionDecisionV1::Replayed => "replayed",
        AutomationWebhookAdmissionDecisionV1::Rejected => "rejected",
    }
}

fn canonical_delivery_decision(
    decision: AutomationWebhookAdmissionDecisionV1,
) -> Result<&'static str, PostgresPersistenceError> {
    match decision {
        AutomationWebhookAdmissionDecisionV1::Admitted => Ok("admitted"),
        AutomationWebhookAdmissionDecisionV1::Rejected => Ok("rejected"),
        AutomationWebhookAdmissionDecisionV1::Replayed => Err(PostgresPersistenceError::Invariant(
            "stored canonical Automation webhook delivery cannot be replayed".into(),
        )),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
