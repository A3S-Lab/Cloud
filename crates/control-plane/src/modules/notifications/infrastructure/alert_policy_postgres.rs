use super::PostgresNotificationRepository;
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::notifications::domain::{
    CreateNotificationAlertPolicyWrite, INotificationAlertPolicyRepository,
    NotificationAlertPolicy, NotificationAlertPolicyCursor, NotificationAlertPolicyDefinition,
    NotificationAlertPolicyTarget, NotificationAlertSource, RevokeNotificationAlertPolicyWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NotificationAlertPolicyId, OrganizationId, PrincipalId,
    RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_POLICIES: &str = "select organization_id, id, recipient_principal_id, source, project_id, environment_id, node_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at from notification_alert_policies";

struct AlertPolicyRow {
    organization_id: Uuid,
    id: Uuid,
    recipient_principal_id: Uuid,
    source: String,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    node_id: Option<Uuid>,
    notify_on_recovery: bool,
    definition_schema: String,
    canonical_acl: String,
    definition_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for AlertPolicyRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            recipient_principal_id: decode(row, 2)?,
            source: decode(row, 3)?,
            project_id: decode(row, 4)?,
            environment_id: decode(row, 5)?,
            node_id: decode(row, 6)?,
            notify_on_recovery: decode(row, 7)?,
            definition_schema: decode(row, 8)?,
            canonical_acl: decode(row, 9)?,
            definition_digest: decode(row, 10)?,
            aggregate_version: decode(row, 11)?,
            created_by: decode(row, 12)?,
            created_at: decode(row, 13)?,
            revoked_at: decode(row, 14)?,
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

fn policy_query() -> SqlQuery<AlertPolicyRow> {
    sql_query::<AlertPolicyRow>(SELECT_POLICIES)
}

fn decode_policy(row: AlertPolicyRow) -> Result<NotificationAlertPolicy, PostgresPersistenceError> {
    let definition =
        NotificationAlertPolicyDefinition::restore(&row.canonical_acl, &row.definition_digest)
            .map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored notification alert policy ACL is invalid: {error}"
                ))
            })?;
    let spec = definition.spec();
    if row.definition_schema != definition.schema()
        || row.source != spec.source.as_str()
        || row.project_id != spec.target.project_id().map(|id| id.as_uuid())
        || row.environment_id != spec.target.environment_id().map(|id| id.as_uuid())
        || row.node_id != spec.target.node_id().map(|id| id.as_uuid())
        || row.notify_on_recovery != spec.notify_on_recovery
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored notification alert policy columns drifted from its ACL".into(),
        ));
    }
    NotificationAlertPolicy::restore(
        OrganizationId::from_uuid(row.organization_id),
        NotificationAlertPolicyId::from_uuid(row.id),
        PrincipalId::from_uuid(row.recipient_principal_id),
        definition,
        row.aggregate_version,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
        row.revoked_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored notification alert policy is invalid: {error}"
        ))
    })
}

#[async_trait]
impl INotificationAlertPolicyRepository for PostgresNotificationRepository {
    async fn replay_alert_policy_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<NotificationAlertPolicy>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    idempotency_replay::<NotificationAlertPolicy>(transaction, &idempotency).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_alert_policy(
        &self,
        write: CreateNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<NotificationAlertPolicy>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let policy = &write.policy;
                    let spec = policy.definition.spec();
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into notification_alert_policies (organization_id, id, recipient_principal_id, source, project_id, environment_id, node_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at) values (",
                        )
                        .bind(policy.organization_id.as_uuid())
                        .append(", ")
                        .bind(policy.id.as_uuid())
                        .append(", ")
                        .bind(policy.recipient_principal_id.as_uuid())
                        .append(", ")
                        .bind(spec.source.as_str())
                        .append(", ")
                        .bind(spec.target.project_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(spec.target.environment_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(spec.target.node_id().map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(spec.notify_on_recovery)
                        .append(", ")
                        .bind(policy.definition.schema())
                        .append(", ")
                        .bind(policy.definition.canonical_acl())
                        .append(", ")
                        .bind(policy.definition.digest().as_str())
                        .append(", ")
                        .bind(policy.aggregate_version)
                        .append(", ")
                        .bind(policy.created_by.as_uuid())
                        .append(", ")
                        .bind(policy.created_at)
                        .append(", null)"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating notification alert policy affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "notification alert policy identity or active source scope is already in use"
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
                            organization_id: policy.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "notification.alert-policy.created",
                            aggregate_id: policy.id.as_uuid(),
                            occurred_at: policy.created_at,
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "policyId": policy.id,
                                "recipientPrincipalId": policy.recipient_principal_id,
                                "definitionDigest": policy.definition.digest(),
                                "source": spec.source.as_str(),
                                "projectId": spec.target.project_id(),
                                "environmentId": spec.target.environment_id(),
                                "nodeId": spec.target.node_id(),
                                "notifyOnRecovery": spec.notify_on_recovery,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, policy).await?;
                    Ok(IdempotentWrite {
                        value: policy.clone(),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_alert_policy(
        &self,
        write: RevokeNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<NotificationAlertPolicy>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<AlertPolicyRow, _>(
                        transaction,
                        policy_query()
                            .append(" where organization_id = ")
                            .bind(write.policy.organization_id.as_uuid())
                            .append(" and recipient_principal_id = ")
                            .bind(write.policy.recipient_principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.policy.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let existing = decode_policy(existing)?;
                    write.validate_against(&existing).map_err(|_| {
                        RepositoryError::Conflict(
                            "notification alert policy changed while revoking".into(),
                        )
                    })?;
                    let policy = &write.policy;
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update notification_alert_policies set aggregate_version = ",
                        )
                        .bind(policy.aggregate_version)
                        .append(", revoked_at = ")
                        .bind(policy.revoked_at)
                        .append(" where organization_id = ")
                        .bind(policy.organization_id.as_uuid())
                        .append(" and recipient_principal_id = ")
                        .bind(policy.recipient_principal_id.as_uuid())
                        .append(" and id = ")
                        .bind(policy.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(write.expected_version)
                        .append(" and revoked_at is null"),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "notification alert policy changed while revoking".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: policy.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "notification.alert-policy.revoked",
                            aggregate_id: policy.id.as_uuid(),
                            occurred_at: policy.revoked_at.expect("validated policy revoke time"),
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "policyId": policy.id,
                                "recipientPrincipalId": policy.recipient_principal_id,
                                "definitionDigest": policy.definition.digest(),
                                "aggregateVersion": policy.aggregate_version,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, policy).await?;
                    Ok(IdempotentWrite {
                        value: policy.clone(),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_alert_policy(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        policy_id: NotificationAlertPolicyId,
    ) -> Result<Option<NotificationAlertPolicy>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                policy_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and recipient_principal_id = ")
                    .bind(recipient_principal_id.as_uuid())
                    .append(" and id = ")
                    .bind(policy_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_policy)
            .transpose()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }

    async fn list_alert_policy_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<NotificationAlertPolicyCursor>,
        limit: usize,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError> {
        let mut query = policy_query()
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
                .bind(after.policy_id.as_uuid())
                .append("))");
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .append(" order by created_at desc, id desc limit ")
                    .bind(limit.max(1)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_policy)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }

    async fn list_active_alert_policies_for_source(
        &self,
        organization_id: OrganizationId,
        source: NotificationAlertSource,
        target: NotificationAlertPolicyTarget,
        occurred_at: DateTime<Utc>,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError> {
        let mut query = policy_query()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and source = ")
            .bind(source.as_str());
        query = match target {
            NotificationAlertPolicyTarget::Environment {
                project_id,
                environment_id,
            } => query
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid()),
            NotificationAlertPolicyTarget::Node { node_id } => {
                query.append(" and node_id = ").bind(node_id.as_uuid())
            }
        };
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .append(" and created_at <= ")
                    .bind(occurred_at)
                    .append(" and revoked_at is null order by created_at, id"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_policy)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}
