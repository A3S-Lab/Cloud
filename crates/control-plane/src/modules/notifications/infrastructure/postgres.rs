use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::notifications::domain::{
    INotificationRepository, MarkNotificationReadWrite, Notification, NotificationAlertSource,
    NotificationCursor, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeId, NotificationId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
    SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct NotificationRow {
    organization_id: Uuid,
    id: Uuid,
    recipient_principal_id: Uuid,
    source_event_id: Uuid,
    source_event_key: String,
    source_schema_version: u32,
    source_aggregate_id: Uuid,
    source_aggregate_version: u64,
    correlation_id: Uuid,
    severity: String,
    title: String,
    body: String,
    scope_kind: String,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    node_id: Option<Uuid>,
    occurred_at: DateTime<Utc>,
    delivered_at: DateTime<Utc>,
    aggregate_version: u64,
    read_at: Option<DateTime<Utc>>,
}

impl FromRow for NotificationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode_column(row, 0)?,
            id: decode_column(row, 1)?,
            recipient_principal_id: decode_column(row, 2)?,
            source_event_id: decode_column(row, 3)?,
            source_event_key: decode_column(row, 4)?,
            source_schema_version: decode_column(row, 5)?,
            source_aggregate_id: decode_column(row, 6)?,
            source_aggregate_version: decode_column(row, 7)?,
            correlation_id: decode_column(row, 8)?,
            severity: decode_column(row, 9)?,
            title: decode_column(row, 10)?,
            body: decode_column(row, 11)?,
            scope_kind: decode_column(row, 12)?,
            project_id: decode_column(row, 13)?,
            environment_id: decode_column(row, 14)?,
            node_id: decode_column(row, 15)?,
            occurred_at: decode_column(row, 16)?,
            delivered_at: decode_column(row, 17)?,
            aggregate_version: decode_column(row, 18)?,
            read_at: decode_column(row, 19)?,
        })
    }
}

fn decode_column<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

#[derive(Clone)]
pub struct PostgresNotificationRepository {
    pub(super) executor: PostgresExecutor,
}

impl PostgresNotificationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl INotificationRepository for PostgresNotificationRepository {
    async fn project(&self, notification: Notification) -> Result<bool, RepositoryError> {
        notification.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing = fetch_optional::<NotificationRow, _>(
                        transaction,
                        notification_query()
                            .append(" where source_event_id = ")
                            .bind(notification.source_event_id)
                            .append(" and recipient_principal_id = ")
                            .bind(notification.recipient_principal_id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .map(decode_notification)
                    .transpose()?;
                    if let Some(existing) = existing {
                        if existing == notification {
                            return Ok(false);
                        }
                        return Err(RepositoryError::Conflict(
                            "notification source event replay changed its immutable projection"
                                .into(),
                        )
                        .into());
                    }

                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into notifications (organization_id, id, recipient_principal_id, source_event_id, source_event_key, source_schema_version, source_aggregate_id, source_aggregate_version, correlation_id, severity, title, body, scope_kind, project_id, environment_id, node_id, occurred_at, delivered_at, aggregate_version, read_at) values (",
                        )
                        .bind(notification.organization_id.as_uuid())
                        .append(", ")
                        .bind(notification.id.as_uuid())
                        .append(", ")
                        .bind(notification.recipient_principal_id.as_uuid())
                        .append(", ")
                        .bind(notification.source_event_id)
                        .append(", ")
                        .bind(notification.source_event_key.as_str())
                        .append(", ")
                        .bind(notification.source_schema_version)
                        .append(", ")
                        .bind(notification.source_aggregate_id)
                        .append(", ")
                        .bind(notification.source_aggregate_version)
                        .append(", ")
                        .bind(notification.correlation_id)
                        .append(", ")
                        .bind(notification.severity.as_str())
                        .append(", ")
                        .bind(notification.title.as_str())
                        .append(", ")
                        .bind(notification.body.as_str())
                        .append(", ")
                        .bind(notification.scope.kind())
                        .append(", ")
                        .bind(notification.scope.project_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(notification.scope.environment_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(notification.scope.node_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(notification.occurred_at)
                        .append(", ")
                        .bind(notification.delivered_at)
                        .append(", ")
                        .bind(notification.aggregate_version)
                        .append(", ")
                        .bind(notification.read_at)
                        .append(") on conflict (source_event_id, recipient_principal_id) do nothing"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {
                            super::outbound_postgres::store_outbound_deliveries(
                                transaction,
                                &notification,
                            )
                            .await?;
                            Ok(true)
                        }
                        Ok(0) => {
                            let existing = fetch_optional::<NotificationRow, _>(
                                transaction,
                                notification_query()
                                    .append(" where source_event_id = ")
                                    .bind(notification.source_event_id)
                                    .append(" and recipient_principal_id = ")
                                    .bind(notification.recipient_principal_id.as_uuid())
                                    .append(" for update"),
                            )
                            .await?
                            .map(decode_notification)
                            .transpose()?
                            .ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "notification conflict disappeared before replay validation"
                                        .into(),
                                )
                            })?;
                            if existing == notification {
                                Ok(false)
                            } else {
                                Err(RepositoryError::Conflict(
                                    "notification source event replay changed its immutable projection"
                                        .into(),
                                )
                                .into())
                            }
                        }
                        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
                            "projecting notification affected {rows} rows"
                        ))),
                        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
                            "notification identity is already in use".into(),
                        )
                        .into()),
                        Err(error) if is_foreign_key_violation(&error) => {
                            Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => Err(error),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        notification_id: NotificationId,
    ) -> Result<Option<Notification>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                notification_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and recipient_principal_id = ")
                    .bind(recipient_principal_id.as_uuid())
                    .append(" and id = ")
                    .bind(notification_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_notification)
            .transpose()
    }

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        unread_only: bool,
        after: Option<NotificationCursor>,
        limit: usize,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let mut query = notification_query()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient_principal_id.as_uuid());
        if unread_only {
            query = query.append(" and read_at is null");
        }
        if let Some(after) = after {
            query = query
                .append(" and (occurred_at < ")
                .bind(after.occurred_at)
                .append(" or (occurred_at = ")
                .bind(after.occurred_at)
                .append(" and id < ")
                .bind(after.notification_id.as_uuid())
                .append("))");
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .append(" order by occurred_at desc, id desc limit ")
                    .bind(limit.max(1)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_notification)
            .collect()
    }

    async fn latest_alert_source_projection(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        source: NotificationAlertSource,
        source_aggregate_id: Uuid,
        not_before: DateTime<Utc>,
        before_aggregate_version: u64,
    ) -> Result<Option<Notification>, RepositoryError> {
        let mut query = notification_query()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient_principal_id.as_uuid())
            .append(" and source_aggregate_id = ")
            .bind(source_aggregate_id)
            .append(" and occurred_at >= ")
            .bind(not_before)
            .append(" and source_aggregate_version < ")
            .bind(before_aggregate_version)
            .append(" and (");
        for (index, event_key) in source.event_keys().iter().enumerate() {
            if index > 0 {
                query = query.append(" or ");
            }
            query = query.append("source_event_key = ").bind(*event_key);
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(query.append(
                ") order by source_aggregate_version desc, occurred_at desc, id desc limit 1",
            ))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_notification)
            .transpose()
    }

    async fn replay_mark_read(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<Notification>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    idempotency_replay::<Notification>(transaction, &idempotency).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn mark_read(
        &self,
        write: MarkNotificationReadWrite,
    ) -> Result<IdempotentWrite<Notification>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<Notification>(transaction, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<NotificationRow, _>(
                        transaction,
                        notification_query()
                            .append(" where organization_id = ")
                            .bind(write.notification.organization_id.as_uuid())
                            .append(" and recipient_principal_id = ")
                            .bind(write.notification.recipient_principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.notification.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .and_then(decode_notification)?;
                    write.validate_against(&existing).map_err(|_| {
                        RepositoryError::Conflict(
                            "notification changed while marking it read".into(),
                        )
                    })?;
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update notifications set aggregate_version = ")
                            .bind(write.notification.aggregate_version)
                            .append(", read_at = ")
                            .bind(write.notification.read_at)
                            .append(" where organization_id = ")
                            .bind(write.notification.organization_id.as_uuid())
                            .append(" and recipient_principal_id = ")
                            .bind(write.notification.recipient_principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.notification.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and read_at is null"),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "notification changed while marking it read".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: write.notification.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "notification.inbox.read",
                            aggregate_id: write.notification.id.as_uuid(),
                            occurred_at: write.notification.read_at.expect("validated read time"),
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "notificationId": write.notification.id,
                                "sourceEventId": write.notification.source_event_id,
                                "aggregateVersion": write.notification.aggregate_version,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.notification).await?;
                    Ok(IdempotentWrite {
                        value: write.notification,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn notification_query() -> SqlQuery<NotificationRow> {
    sql_query::<NotificationRow>(
        "select organization_id, id, recipient_principal_id, source_event_id, source_event_key, source_schema_version, source_aggregate_id, source_aggregate_version, correlation_id, severity, title, body, scope_kind, project_id, environment_id, node_id, occurred_at, delivered_at, aggregate_version, read_at from notifications",
    )
}

fn decode_notification(row: NotificationRow) -> Result<Notification, RepositoryError> {
    let scope = match (
        row.scope_kind.as_str(),
        row.project_id,
        row.environment_id,
        row.node_id,
    ) {
        ("organization", None, None, None) => NotificationScope::Organization,
        ("project", Some(project_id), None, None) => NotificationScope::Project {
            project_id: ProjectId::from_uuid(project_id),
        },
        ("environment", Some(project_id), Some(environment_id), None) => {
            NotificationScope::Environment {
                project_id: ProjectId::from_uuid(project_id),
                environment_id: EnvironmentId::from_uuid(environment_id),
            }
        }
        ("node", None, None, Some(node_id)) => NotificationScope::Node {
            node_id: NodeId::from_uuid(node_id),
        },
        _ => {
            return Err(RepositoryError::Storage(
                "stored notification scope is invalid".into(),
            ))
        }
    };
    let notification = Notification {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        id: NotificationId::from_uuid(row.id),
        recipient_principal_id: PrincipalId::from_uuid(row.recipient_principal_id),
        source_event_id: row.source_event_id,
        source_event_key: row.source_event_key,
        source_schema_version: row.source_schema_version,
        source_aggregate_id: row.source_aggregate_id,
        source_aggregate_version: row.source_aggregate_version,
        correlation_id: row.correlation_id,
        severity: NotificationSeverity::parse(&row.severity).map_err(RepositoryError::Storage)?,
        title: row.title,
        body: row.body,
        scope,
        occurred_at: row.occurred_at,
        delivered_at: row.delivered_at,
        aggregate_version: row.aggregate_version,
        read_at: row.read_at,
    };
    notification.validate().map_err(RepositoryError::Storage)?;
    Ok(notification)
}
