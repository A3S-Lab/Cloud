use super::postgres::PostgresEdgeRepository;
use super::postgres_mcp_credentials::{insert_credential, lock_credential, transition_credential};
use super::postgres_schema::McpCredentialDeliveries;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    IMcpCredentialLifecycleRepository, McpCredentialLifecycleReference,
    McpCredentialLifecycleResult, StoreMcpCredentialLifecycle,
    MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH,
};
use crate::modules::edge::domain::McpCredentialDelivery;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    delete_from, insert_into, select_from, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IMcpCredentialLifecycleRepository for PostgresEdgeRepository {
    async fn replay_mcp_credential_lifecycle(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        idempotency: &IdempotencyRequest,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError> {
        replay(
            &self.executor,
            organization_id,
            project_id,
            environment_id,
            idempotency.clone(),
            canonical_timestamp(observed_at),
        )
        .await
    }

    async fn store_mcp_credential_lifecycle(
        &self,
        bundle: StoreMcpCredentialLifecycle,
    ) -> Result<McpCredentialLifecycleResult, RepositoryError> {
        store(&self.executor, bundle).await
    }

    async fn purge_expired_mcp_credential_deliveries(
        &self,
        observed_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        purge(&self.executor, canonical_timestamp(observed_at), limit).await
    }
}

async fn replay(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    idempotency: IdempotencyRequest,
    observed_at: DateTime<Utc>,
) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(reference) = idempotency_replay::<McpCredentialLifecycleReference>(
                    transaction,
                    &idempotency,
                )
                .await?
                .map(|replay| replay.value) else {
                    return Ok(None);
                };
                if reference.organization_id != organization_id
                    || reference.project_id != project_id
                    || reference.environment_id != environment_id
                {
                    return Err(PostgresPersistenceError::from(RepositoryError::NotFound));
                }
                Ok(Some(
                    resolve_reference(transaction, reference, observed_at).await?,
                ))
            })
        })
        .await
        .map_err(transaction_error)
}

async fn store(
    executor: &PostgresExecutor,
    bundle: StoreMcpCredentialLifecycle,
) -> Result<McpCredentialLifecycleResult, RepositoryError> {
    bundle.validate()?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(reference) = idempotency_replay::<McpCredentialLifecycleReference>(
                    transaction,
                    &bundle.idempotency,
                )
                .await?
                .map(|replay| replay.value)
                {
                    if reference.organization_id != bundle.credential.organization_id
                        || reference.project_id != bundle.credential.project_id
                        || reference.environment_id != bundle.credential.environment_id
                    {
                        return Err(PostgresPersistenceError::from(RepositoryError::NotFound));
                    }
                    return resolve_reference(transaction, reference, bundle.observed_at).await;
                }

                match bundle.expected_aggregate_version {
                    None => insert_credential(transaction, &bundle.credential).await?,
                    Some(expected_version) => {
                        transition_credential(transaction, &bundle.credential, expected_version)
                            .await?
                    }
                }
                delete_current_delivery(transaction, bundle.credential.id).await?;
                if let Some(delivery) = &bundle.delivery {
                    insert_delivery(transaction, delivery).await?;
                }
                store_outbox(transaction, &bundle.event).await?;
                store_audit(transaction, &bundle.audit).await?;
                let reference = McpCredentialLifecycleReference::from_bundle(&bundle);
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(McpCredentialLifecycleResult {
                    credential: bundle.credential,
                    delivery: bundle.delivery,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn resolve_reference(
    transaction: &a3s_orm::PostgresTransaction,
    reference: McpCredentialLifecycleReference,
    observed_at: DateTime<Utc>,
) -> Result<McpCredentialLifecycleResult, PostgresPersistenceError> {
    let credential = lock_credential(
        transaction,
        reference.organization_id,
        reference.credential_id,
    )
    .await?;
    if !reference.matches_credential(&credential) {
        return Err(RepositoryError::Conflict(
            "MCP credential lifecycle advanced beyond this idempotent request".into(),
        )
        .into());
    }
    let delivery = fetch_optional::<McpCredentialDeliveryRow, _>(
        transaction,
        delivery_query(reference.credential_id).for_update(),
    )
    .await?
    .map(McpCredentialDeliveryRow::delivery)
    .transpose()?;
    let delivery = match (reference.has_delivery, delivery) {
        (true, Some(delivery))
            if delivery.matches_credential(&credential)
                && delivery.is_available_at(canonical_timestamp(observed_at)) =>
        {
            Some(delivery)
        }
        (true, _) => {
            return Err(RepositoryError::Conflict(
                "MCP credential recovery window is no longer available".into(),
            )
            .into())
        }
        (false, None) => None,
        (false, Some(_)) => {
            return Err(PostgresPersistenceError::Invariant(
                "revoked MCP credential retained recovery material".into(),
            ))
        }
    };
    Ok(McpCredentialLifecycleResult {
        credential,
        delivery,
        replayed: true,
    })
}

async fn insert_delivery(
    transaction: &a3s_orm::PostgresTransaction,
    delivery: &McpCredentialDelivery,
) -> Result<(), PostgresPersistenceError> {
    delivery.validate().map_err(RepositoryError::Conflict)?;
    let inserted = execute(
        transaction,
        insert_into::<McpCredentialDeliveries>()
            .value(
                McpCredentialDeliveries::credential_id(),
                delivery.credential_id().as_uuid(),
            )
            .value(
                McpCredentialDeliveries::organization_id(),
                delivery.organization_id().as_uuid(),
            )
            .value(
                McpCredentialDeliveries::project_id(),
                delivery.project_id().as_uuid(),
            )
            .value(
                McpCredentialDeliveries::environment_id(),
                delivery.environment_id().as_uuid(),
            )
            .value(McpCredentialDeliveries::generation(), delivery.generation())
            .value(McpCredentialDeliveries::key_id(), delivery.key_id())
            .value(McpCredentialDeliveries::ciphertext(), delivery.ciphertext())
            .value(McpCredentialDeliveries::created_at(), delivery.created_at())
            .value(McpCredentialDeliveries::expires_at(), delivery.expires_at()),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("MCP credential recovery delivery", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "MCP credential recovery delivery already exists".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn delete_current_delivery(
    transaction: &a3s_orm::PostgresTransaction,
    credential_id: McpCredentialId,
) -> Result<u64, PostgresPersistenceError> {
    execute(
        transaction,
        delete_from::<McpCredentialDeliveries>()
            .filter(McpCredentialDeliveries::credential_id().eq(credential_id.as_uuid())),
    )
    .await
}

async fn purge(
    executor: &PostgresExecutor,
    observed_at: DateTime<Utc>,
    limit: usize,
) -> Result<usize, RepositoryError> {
    if limit == 0 || limit > MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH {
        return Err(RepositoryError::Conflict(
            "MCP credential delivery purge limit must be between 1 and 10000".into(),
        ));
    }
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict(
            "MCP credential delivery purge limit exceeds supported range".into(),
        )
    })?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let expired = fetch_all::<(Uuid, u64, DateTime<Utc>), _>(
                    transaction,
                    select_from::<McpCredentialDeliveries>()
                        .select((
                            McpCredentialDeliveries::credential_id(),
                            McpCredentialDeliveries::generation(),
                            McpCredentialDeliveries::expires_at(),
                        ))
                        .filter(McpCredentialDeliveries::expires_at().lte(observed_at))
                        .order_by(McpCredentialDeliveries::expires_at(), OrderDirection::Asc)
                        .order_by(
                            McpCredentialDeliveries::credential_id(),
                            OrderDirection::Asc,
                        )
                        .limit(limit)
                        .for_update(),
                )
                .await?;
                for (credential_id, generation, expires_at) in &expired {
                    let deleted = execute(
                        transaction,
                        delete_from::<McpCredentialDeliveries>()
                            .filter(McpCredentialDeliveries::credential_id().eq(*credential_id))
                            .filter(McpCredentialDeliveries::generation().eq(*generation))
                            .filter(McpCredentialDeliveries::expires_at().eq(*expires_at)),
                    )
                    .await?;
                    require_one_row("expired MCP credential recovery delivery", deleted)?;
                }
                Ok(expired.len())
            })
        })
        .await
        .map_err(transaction_error)
}

fn delivery_query(
    credential_id: McpCredentialId,
) -> a3s_orm::query::SelectQuery<McpCredentialDeliveries, McpCredentialDeliveryRow> {
    select_from::<McpCredentialDeliveries>()
        .select(McpCredentialDeliverySelection)
        .filter(McpCredentialDeliveries::credential_id().eq(credential_id.as_uuid()))
}

struct McpCredentialDeliveryRow {
    credential_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    generation: u64,
    key_id: String,
    ciphertext: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

struct McpCredentialDeliverySelection;

impl Selection for McpCredentialDeliverySelection {
    type Output = McpCredentialDeliveryRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            McpCredentialDeliveries::credential_id().expression(),
            McpCredentialDeliveries::organization_id().expression(),
            McpCredentialDeliveries::project_id().expression(),
            McpCredentialDeliveries::environment_id().expression(),
            McpCredentialDeliveries::generation().expression(),
            McpCredentialDeliveries::key_id().expression(),
            McpCredentialDeliveries::ciphertext().expression(),
            McpCredentialDeliveries::created_at().expression(),
            McpCredentialDeliveries::expires_at().expression(),
        ]
    }
}

impl FromRow for McpCredentialDeliveryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            credential_id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            generation: decode(row, 4)?,
            key_id: decode(row, 5)?,
            ciphertext: decode(row, 6)?,
            created_at: decode(row, 7)?,
            expires_at: decode(row, 8)?,
        })
    }
}

impl McpCredentialDeliveryRow {
    fn delivery(self) -> Result<McpCredentialDelivery, PostgresPersistenceError> {
        McpCredentialDelivery::new(
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            EnvironmentId::from_uuid(self.environment_id),
            McpCredentialId::from_uuid(self.credential_id),
            self.generation,
            self.key_id,
            self.ciphertext,
            self.created_at,
            self.expires_at,
        )
        .map_err(|error| {
            RepositoryError::Storage(format!(
                "stored MCP credential recovery delivery is invalid: {error}"
            ))
            .into()
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

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn recovery_lookup_is_exactly_bound_to_one_credential() {
        let credential_id = McpCredentialId::new();
        let compiled = delivery_query(credential_id)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 1);
        assert!(compiled.sql.contains("\"credential_id\" = $1"));
        assert_eq!(
            compiled.parameters[0],
            a3s_orm::Value::Uuid(credential_id.as_uuid())
        );
    }
}
