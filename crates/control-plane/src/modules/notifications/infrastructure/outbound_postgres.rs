use super::PostgresNotificationRepository;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, store_audit, store_idempotency, store_outbox, transaction_error,
    AuditWrite, PostgresPersistenceError,
};
use crate::modules::notifications::domain::{
    CreateOutboundNotificationSubscriptionWrite, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationRepository, Notification, OutboundNotificationConnectorTarget,
    OutboundNotificationDelivery, OutboundNotificationDeliveryAdmission,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionCursor,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationTerminalOutcome,
    OutboundNotificationTerminalReceipt, RevokeOutboundNotificationSubscriptionWrite,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresTransaction,
    Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_SUBSCRIPTIONS: &str = "select organization_id, id, recipient_principal_id, channel, minimum_severity, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, definition_schema, maximum_provider_attempts, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at from notification_outbound_subscriptions";
const SELECT_DELIVERIES: &str = "select organization_id, id, notification_id, recipient_principal_id, subscription_id, requested_event_id, payload_digest, maximum_provider_attempts, channel, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, occurred_at, terminal_outcome, terminal_generation, terminal_attempt_id, terminal_at from notification_outbound_deliveries";

struct OutboundSubscriptionRow {
    organization_id: Uuid,
    id: Uuid,
    recipient_principal_id: Uuid,
    channel: String,
    minimum_severity: String,
    connector_project_id: Uuid,
    connector_environment_id: Uuid,
    connector_profile_id: Uuid,
    connector_revision_id: Uuid,
    definition_schema: String,
    maximum_provider_attempts: u64,
    canonical_acl: String,
    definition_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for OutboundSubscriptionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            recipient_principal_id: decode(row, 2)?,
            channel: decode(row, 3)?,
            minimum_severity: decode(row, 4)?,
            connector_project_id: decode(row, 5)?,
            connector_environment_id: decode(row, 6)?,
            connector_profile_id: decode(row, 7)?,
            connector_revision_id: decode(row, 8)?,
            definition_schema: decode(row, 9)?,
            maximum_provider_attempts: decode(row, 10)?,
            canonical_acl: decode(row, 11)?,
            definition_digest: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
            created_by: decode(row, 14)?,
            created_at: decode(row, 15)?,
            revoked_at: decode(row, 16)?,
        })
    }
}

struct OutboundDeliveryRow {
    organization_id: Uuid,
    id: Uuid,
    notification_id: Uuid,
    recipient_principal_id: Uuid,
    subscription_id: Uuid,
    requested_event_id: Uuid,
    payload_digest: String,
    maximum_provider_attempts: u64,
    channel: String,
    connector_project_id: Uuid,
    connector_environment_id: Uuid,
    connector_profile_id: Uuid,
    connector_revision_id: Uuid,
    occurred_at: DateTime<Utc>,
    terminal_outcome: Option<String>,
    terminal_generation: Option<u64>,
    terminal_attempt_id: Option<Uuid>,
    terminal_at: Option<DateTime<Utc>>,
}

impl FromRow for OutboundDeliveryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            notification_id: decode(row, 2)?,
            recipient_principal_id: decode(row, 3)?,
            subscription_id: decode(row, 4)?,
            requested_event_id: decode(row, 5)?,
            payload_digest: decode(row, 6)?,
            maximum_provider_attempts: decode(row, 7)?,
            channel: decode(row, 8)?,
            connector_project_id: decode(row, 9)?,
            connector_environment_id: decode(row, 10)?,
            connector_profile_id: decode(row, 11)?,
            connector_revision_id: decode(row, 12)?,
            occurred_at: decode(row, 13)?,
            terminal_outcome: decode(row, 14)?,
            terminal_generation: decode(row, 15)?,
            terminal_attempt_id: decode(row, 16)?,
            terminal_at: decode(row, 17)?,
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

pub(super) async fn store_outbound_deliveries(
    transaction: &PostgresTransaction,
    notification: &Notification,
) -> Result<(), PostgresPersistenceError> {
    let subscriptions = fetch_all::<OutboundSubscriptionRow, _>(
        transaction,
        subscription_query()
            .append(" where organization_id = ")
            .bind(notification.organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(notification.recipient_principal_id.as_uuid())
            .append(" and revoked_at is null order by created_at, id for share"),
    )
    .await?;
    for row in subscriptions {
        let subscription = decode_subscription(row)?;
        if !subscription.matches(notification) {
            continue;
        }
        let spec = subscription.definition.spec();
        let delivery = subscription
            .definition
            .delivery_for(notification)
            .map_err(RepositoryError::Storage)?;
        let payload_digest = Sha256Digest::from_bytes(
            &delivery
                .canonical_payload()
                .map_err(RepositoryError::Storage)?,
        );
        let event = delivery
            .requested_event()
            .map_err(RepositoryError::Storage)?;
        store_outbox(transaction, &event).await?;
        let inserted = execute(
            transaction,
            sql_query::<()>(
                "insert into notification_outbound_deliveries (organization_id, id, notification_id, recipient_principal_id, subscription_id, requested_event_id, payload_digest, maximum_provider_attempts, channel, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, occurred_at, terminal_outcome, terminal_generation, terminal_attempt_id, terminal_at) values (",
            )
            .bind(delivery.organization_id().as_uuid())
            .append(", ")
            .bind(delivery.id())
            .append(", ")
            .bind(delivery.notification_id().as_uuid())
            .append(", ")
            .bind(delivery.recipient_principal_id().as_uuid())
            .append(", ")
            .bind(subscription.id.as_uuid())
            .append(", ")
            .bind(delivery.requested_event_id())
            .append(", ")
            .bind(payload_digest.as_str())
            .append(", ")
            .bind(delivery.maximum_provider_attempts())
            .append(", ")
            .bind(delivery.channel().as_str())
            .append(", ")
            .bind(spec.target.project_id.as_uuid())
            .append(", ")
            .bind(spec.target.environment_id.as_uuid())
            .append(", ")
            .bind(spec.target.profile_id.as_uuid())
            .append(", ")
            .bind(spec.target.revision_id.as_uuid())
            .append(", ")
            .bind(delivery.occurred_at())
            .append(", null, null, null, null)"),
        )
        .await?;
        if inserted != 1 {
            return Err(PostgresPersistenceError::Invariant(format!(
                "storing outbound notification delivery affected {inserted} rows"
            )));
        }
    }
    Ok(())
}

fn subscription_query() -> SqlQuery<OutboundSubscriptionRow> {
    sql_query::<OutboundSubscriptionRow>(SELECT_SUBSCRIPTIONS)
}

fn delivery_query() -> SqlQuery<OutboundDeliveryRow> {
    sql_query::<OutboundDeliveryRow>(SELECT_DELIVERIES)
}

fn decode_subscription(
    row: OutboundSubscriptionRow,
) -> Result<OutboundNotificationSubscription, PostgresPersistenceError> {
    let definition = OutboundNotificationSubscriptionDefinition::restore(
        &row.canonical_acl,
        &row.definition_digest,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored outbound notification subscription ACL is invalid: {error}"
        ))
    })?;
    let spec = definition.spec();
    if row.definition_schema != definition.definition_schema()
        || row.maximum_provider_attempts != definition.maximum_provider_attempts()
        || row.channel != spec.channel.as_str()
        || row.minimum_severity != spec.minimum_severity.as_str()
        || row.connector_project_id != spec.target.project_id.as_uuid()
        || row.connector_environment_id != spec.target.environment_id.as_uuid()
        || row.connector_profile_id != spec.target.profile_id.as_uuid()
        || row.connector_revision_id != spec.target.revision_id.as_uuid()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored outbound notification subscription columns drifted from its ACL".into(),
        ));
    }
    OutboundNotificationSubscription::restore(
        OrganizationId::from_uuid(row.organization_id),
        NotificationSubscriptionId::from_uuid(row.id),
        PrincipalId::from_uuid(row.recipient_principal_id),
        definition,
        row.aggregate_version,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
        row.revoked_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored outbound notification subscription is invalid: {error}"
        ))
    })
}

fn delivery_matches_row(
    delivery: &OutboundNotificationDelivery,
    row: &OutboundDeliveryRow,
) -> Result<bool, PostgresPersistenceError> {
    let target = delivery.target();
    let payload_digest = Sha256Digest::from_bytes(
        &delivery
            .canonical_payload()
            .map_err(RepositoryError::Storage)?,
    );
    Ok(row.organization_id == delivery.organization_id().as_uuid()
        && row.id == delivery.id()
        && row.notification_id == delivery.notification_id().as_uuid()
        && row.recipient_principal_id == delivery.recipient_principal_id().as_uuid()
        && !row.subscription_id.is_nil()
        && row.requested_event_id == delivery.requested_event_id()
        && row.payload_digest == payload_digest.as_str()
        && row.maximum_provider_attempts == delivery.maximum_provider_attempts()
        && row.channel == delivery.channel().as_str()
        && row.connector_project_id == target.project_id.as_uuid()
        && row.connector_environment_id == target.environment_id.as_uuid()
        && row.connector_profile_id == target.profile_id.as_uuid()
        && row.connector_revision_id == target.revision_id.as_uuid()
        && row.occurred_at == delivery.occurred_at())
}

fn decode_receipt(
    row: &OutboundDeliveryRow,
) -> Result<Option<OutboundNotificationTerminalReceipt>, PostgresPersistenceError> {
    match (
        row.terminal_outcome.as_deref(),
        row.terminal_generation,
        row.terminal_attempt_id,
        row.terminal_at,
    ) {
        (None, None, None, None) => Ok(None),
        (Some(outcome), Some(generation), Some(attempt_id), Some(terminal_at)) => {
            OutboundNotificationTerminalReceipt::restore_with_provider_attempt_budget(
                OrganizationId::from_uuid(row.organization_id),
                row.id,
                OutboundNotificationConnectorTarget::new(
                    ProjectId::from_uuid(row.connector_project_id),
                    EnvironmentId::from_uuid(row.connector_environment_id),
                    ConnectorProfileId::from_uuid(row.connector_profile_id),
                    ConnectorRevisionId::from_uuid(row.connector_revision_id),
                )
                .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?,
                row.maximum_provider_attempts,
                OutboundNotificationTerminalOutcome::parse(outcome)
                    .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?,
                generation,
                attempt_id,
                terminal_at,
            )
            .map(Some)
            .map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored outbound notification terminal receipt is invalid: {error}"
                ))
            })
        }
        _ => Err(PostgresPersistenceError::Invariant(
            "stored outbound notification terminal receipt is incomplete".into(),
        )),
    }
}

#[async_trait]
impl IOutboundNotificationRepository for PostgresNotificationRepository {
    async fn replay_subscription_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<OutboundNotificationSubscription>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    idempotency_replay::<OutboundNotificationSubscription>(
                        transaction,
                        &idempotency,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_subscription(
        &self,
        write: CreateOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<
                        OutboundNotificationSubscription,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let subscription = &write.subscription;
                    let spec = subscription.definition.spec();
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into notification_outbound_subscriptions (organization_id, id, recipient_principal_id, channel, minimum_severity, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, definition_schema, maximum_provider_attempts, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at) values (",
                        )
                        .bind(subscription.organization_id.as_uuid())
                        .append(", ")
                        .bind(subscription.id.as_uuid())
                        .append(", ")
                        .bind(subscription.recipient_principal_id.as_uuid())
                        .append(", ")
                        .bind(spec.channel.as_str())
                        .append(", ")
                        .bind(spec.minimum_severity.as_str())
                        .append(", ")
                        .bind(spec.target.project_id.as_uuid())
                        .append(", ")
                        .bind(spec.target.environment_id.as_uuid())
                        .append(", ")
                        .bind(spec.target.profile_id.as_uuid())
                        .append(", ")
                        .bind(spec.target.revision_id.as_uuid())
                        .append(", ")
                        .bind(subscription.definition.definition_schema())
                        .append(", ")
                        .bind(subscription.definition.maximum_provider_attempts())
                        .append(", ")
                        .bind(subscription.definition.canonical_acl())
                        .append(", ")
                        .bind(subscription.definition.digest().as_str())
                        .append(", ")
                        .bind(subscription.aggregate_version)
                        .append(", ")
                        .bind(subscription.created_by.as_uuid())
                        .append(", ")
                        .bind(subscription.created_at)
                        .append(", null)"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating outbound notification subscription affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "outbound notification subscription identity or active target is already in use"
                                    .into(),
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
                            organization_id: subscription.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "notification.outbound-subscription.created",
                            aggregate_id: subscription.id.as_uuid(),
                            occurred_at: subscription.created_at,
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "subscriptionId": subscription.id,
                                "recipientPrincipalId": subscription.recipient_principal_id,
                                "definitionDigest": subscription.definition.digest(),
                                "definitionSchema": subscription.definition.definition_schema(),
                                "maximumProviderAttempts": subscription.definition.maximum_provider_attempts(),
                                "channel": spec.channel,
                                "connectorProjectId": spec.target.project_id,
                                "connectorEnvironmentId": spec.target.environment_id,
                                "connectorProfileId": spec.target.profile_id,
                                "connectorRevisionId": spec.target.revision_id,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, subscription).await?;
                    Ok(IdempotentWrite {
                        value: subscription.clone(),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_subscription(
        &self,
        write: RevokeOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<OutboundNotificationSubscription>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<OutboundSubscriptionRow, _>(
                        transaction,
                        subscription_query()
                            .append(" where organization_id = ")
                            .bind(write.subscription.organization_id.as_uuid())
                            .append(" and recipient_principal_id = ")
                            .bind(write.subscription.recipient_principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.subscription.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let existing = decode_subscription(existing)?;
                    write.validate_against(&existing).map_err(|_| {
                        RepositoryError::Conflict(
                            "outbound notification subscription changed while revoking".into(),
                        )
                    })?;
                    let subscription = &write.subscription;
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update notification_outbound_subscriptions set aggregate_version = ",
                        )
                        .bind(subscription.aggregate_version)
                        .append(", revoked_at = ")
                        .bind(subscription.revoked_at)
                        .append(" where organization_id = ")
                        .bind(subscription.organization_id.as_uuid())
                        .append(" and recipient_principal_id = ")
                        .bind(subscription.recipient_principal_id.as_uuid())
                        .append(" and id = ")
                        .bind(subscription.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(write.expected_version)
                        .append(" and revoked_at is null"),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "outbound notification subscription changed while revoking".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: subscription.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "notification.outbound-subscription.revoked",
                            aggregate_id: subscription.id.as_uuid(),
                            occurred_at: subscription
                                .revoked_at
                                .expect("validated subscription revoke time"),
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "subscriptionId": subscription.id,
                                "recipientPrincipalId": subscription.recipient_principal_id,
                                "definitionDigest": subscription.definition.digest(),
                                "aggregateVersion": subscription.aggregate_version,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, subscription).await?;
                    Ok(IdempotentWrite {
                        value: subscription.clone(),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_subscription(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        subscription_id: NotificationSubscriptionId,
    ) -> Result<Option<OutboundNotificationSubscription>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                subscription_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and recipient_principal_id = ")
                    .bind(recipient_principal_id.as_uuid())
                    .append(" and id = ")
                    .bind(subscription_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_subscription)
            .transpose()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }

    async fn list_subscription_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<OutboundNotificationSubscriptionCursor>,
        limit: usize,
    ) -> Result<Vec<OutboundNotificationSubscription>, RepositoryError> {
        let mut query = subscription_query()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient_principal_id.as_uuid());
        if let Some(after) = after {
            query = query
                .append(" and (created_at < ")
                .bind(after.created_at)
                .append(" or (created_at = ")
                .bind(after.created_at)
                .append(" and id < ")
                .bind(after.subscription_id.as_uuid())
                .append("))");
        }
        query = query
            .append(" order by created_at desc, id desc limit ")
            .bind(i64::try_from(limit).unwrap_or(i64::MAX));
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(query)
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_subscription)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}

#[async_trait]
impl IOutboundNotificationDeliveryRepository for PostgresNotificationRepository {
    async fn admit_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<Option<OutboundNotificationDeliveryAdmission>, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                delivery_query()
                    .append(" where organization_id = ")
                    .bind(delivery.organization_id().as_uuid())
                    .append(" and id = ")
                    .bind(delivery.id()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let Some(row) = row else {
            return Ok(None);
        };
        if !delivery_matches_row(delivery, &row)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
        {
            return Ok(None);
        }
        let receipt =
            decode_receipt(&row).map_err(|error| RepositoryError::Storage(error.to_string()))?;
        Ok(Some(match receipt {
            Some(receipt) => OutboundNotificationDeliveryAdmission::Terminal(receipt),
            None => OutboundNotificationDeliveryAdmission::Pending,
        }))
    }

    async fn settle_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: OutboundNotificationTerminalReceipt,
    ) -> Result<bool, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        receipt
            .validate_against(delivery)
            .map_err(RepositoryError::Storage)?;
        let delivery = delivery.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<OutboundDeliveryRow, _>(
                        transaction,
                        delivery_query()
                            .append(" where organization_id = ")
                            .bind(delivery.organization_id().as_uuid())
                            .append(" and id = ")
                            .bind(delivery.id())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if !delivery_matches_row(&delivery, &row)? {
                        return Err(RepositoryError::Conflict(
                            "outbound notification delivery fact changed before settlement".into(),
                        )
                        .into());
                    }
                    if let Some(existing) = decode_receipt(&row)? {
                        if existing == receipt {
                            return Ok(false);
                        }
                        return Err(RepositoryError::Conflict(
                            "outbound notification delivery already has another terminal receipt"
                                .into(),
                        )
                        .into());
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update notification_outbound_deliveries set terminal_outcome = ",
                        )
                        .bind(receipt.outcome().as_str())
                        .append(", terminal_generation = ")
                        .bind(receipt.generation())
                        .append(", terminal_attempt_id = ")
                        .bind(receipt.attempt_id())
                        .append(", terminal_at = ")
                        .bind(receipt.terminal_at())
                        .append(" where organization_id = ")
                        .bind(delivery.organization_id().as_uuid())
                        .append(" and id = ")
                        .bind(delivery.id())
                        .append(" and terminal_outcome is null"),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "outbound notification delivery changed while settling".into(),
                        )
                        .into());
                    }
                    Ok(true)
                })
            })
            .await
            .map_err(transaction_error)
    }
}
