use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_unique_violation, store_idempotency,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{ApiToken, IdentityBootstrap, Organization};
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IOrganizationRepository,
};
use crate::modules::identity::domain::value_objects::{
    ApiTokenDigest, ApiTokenName, ApiTokenScope, OrganizationName,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresIdentityRepository {
    executor: PostgresExecutor,
}

impl PostgresIdentityRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IOrganizationRepository for PostgresIdentityRepository {
    async fn create(
        &self,
        organization: Organization,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<Organization>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(organization.id.as_uuid())
                        .append(", ")
                        .bind(organization.name.as_str())
                        .append(", ")
                        .bind(organization.name.key())
                        .append(", ")
                        .bind(organization.aggregate_version)
                        .append(", ")
                        .bind(organization.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating organization affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "organization name is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_idempotency(transaction, &idempotency, &organization).await?;
                    Ok(IdempotentWrite {
                        value: organization,
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
    ) -> Result<Option<Organization>, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<(Uuid, String, u64, DateTime<Utc>)>(
                    "select id, name, aggregate_version, created_at from organizations where id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(|(id, name, aggregate_version, created_at)| {
            let name = OrganizationName::parse(name).map_err(|error| {
                RepositoryError::Storage(format!("stored organization name is invalid: {error}"))
            })?;
            Ok(Organization {
                id: OrganizationId::from_uuid(id),
                name,
                aggregate_version,
                created_at,
            })
        })
        .transpose()
    }

    async fn list(&self) -> Result<Vec<Organization>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(sql_query::<(Uuid, String, u64, DateTime<Utc>)>(
                "select id, name, aggregate_version, created_at from organizations order by created_at asc, id asc",
            ))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(|(id, name, aggregate_version, created_at)| {
                let name = OrganizationName::parse(name).map_err(|error| {
                    RepositoryError::Storage(format!(
                        "stored organization name is invalid: {error}"
                    ))
                })?;
                Ok(Organization {
                    id: OrganizationId::from_uuid(id),
                    name,
                    aggregate_version,
                    created_at,
                })
            })
            .collect()
    }
}

type ApiTokenRow = (
    Uuid,
    Uuid,
    String,
    serde_json::Value,
    u64,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

fn decode_token(row: ApiTokenRow) -> Result<ApiToken, RepositoryError> {
    let (id, organization_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at) =
        row;
    let name = ApiTokenName::parse(name).map_err(|error| {
        RepositoryError::Storage(format!("stored API token name is invalid: {error}"))
    })?;
    let scopes = serde_json::from_value::<Vec<String>>(scopes)
        .map_err(|error| {
            RepositoryError::Storage(format!("stored API token scopes are invalid: {error}"))
        })?
        .into_iter()
        .map(ApiTokenScope::parse)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| {
            RepositoryError::Storage(format!("stored API token scope is invalid: {error}"))
        })?;
    Ok(ApiToken {
        id: ApiTokenId::from_uuid(id),
        organization_id: OrganizationId::from_uuid(organization_id),
        name,
        scopes,
        aggregate_version,
        created_at,
        expires_at,
        revoked_at,
    })
}

async fn insert_token(
    transaction: &a3s_orm::PostgresTransaction,
    token: &ApiToken,
    digest: &ApiTokenDigest,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into api_tokens (id, organization_id, name, name_key, token_hash, scopes, aggregate_version, created_at, expires_at, revoked_at) values (",
        )
        .bind(token.id.as_uuid())
        .append(", ")
        .bind(token.organization_id.as_uuid())
        .append(", ")
        .bind(token.name.as_str())
        .append(", ")
        .bind(token.name.key())
        .append(", ")
        .bind(digest.as_str())
        .append(", ")
        .bind(serde_json::to_value(&token.scopes)?)
        .append(", ")
        .bind(token.aggregate_version)
        .append(", ")
        .bind(token.created_at)
        .append(", ")
        .bind(token.expires_at)
        .append(", ")
        .bind(token.revoked_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating API token affected {rows} rows"
        )));
    }
    Ok(())
}

#[async_trait]
impl IApiTokenRepository for PostgresIdentityRepository {
    async fn bootstrap(
        &self,
        bootstrap: IdentityBootstrap,
        digest: ApiTokenDigest,
        events: [DomainEventEnvelope; 2],
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<IdentityBootstrap>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let locked = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from (select pg_advisory_xact_lock(hashtext(",
                        )
                        .bind("a3s-cloud.identity.bootstrap")
                        .append("))) as locked"),
                    )
                    .await?;
                    if locked != Some(1) {
                        return Err(PostgresPersistenceError::Invariant(
                            "identity bootstrap lock did not return a row".into(),
                        ));
                    }
                    let organization_count = fetch_optional::<i64, _>(
                        transaction,
                        sql_query::<i64>("select count(*) from organizations"),
                    )
                    .await?
                    .unwrap_or_default();
                    if organization_count != 0 {
                        return Err(RepositoryError::Conflict(
                            "Cloud identity has already been bootstrapped".into(),
                        )
                        .into());
                    }
                    let organization = &bootstrap.organization;
                    let organization_rows = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(organization.id.as_uuid())
                        .append(", ")
                        .bind(organization.name.as_str())
                        .append(", ")
                        .bind(organization.name.key())
                        .append(", ")
                        .bind(organization.aggregate_version)
                        .append(", ")
                        .bind(organization.created_at)
                        .append(")"),
                    )
                    .await?;
                    if organization_rows != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "bootstrapping organization affected {organization_rows} rows"
                        )));
                    }
                    insert_token(transaction, &bootstrap.api_token, &digest).await?;
                    for event in &events {
                        store_outbox(transaction, event).await?;
                    }
                    store_idempotency(transaction, &idempotency, &bootstrap).await?;
                    Ok(IdempotentWrite {
                        value: bootstrap,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create(
        &self,
        token: ApiToken,
        digest: ApiTokenDigest,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<ApiToken>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let organization_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(token.organization_id.as_uuid()),
                    )
                    .await?
                    .is_some();
                    if !organization_exists {
                        return Err(RepositoryError::NotFound.into());
                    }
                    match insert_token(transaction, &token, &digest).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "API token name or credential is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_idempotency(transaction, &idempotency, &token).await?;
                    Ok(IdempotentWrite {
                        value: token,
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
        token_id: ApiTokenId,
    ) -> Result<Option<ApiToken>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ApiTokenRow>(
                    "select id, organization_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(token_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_token)
            .transpose()
    }

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<ApiToken>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ApiTokenRow>(
                    "select id, organization_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where token_hash = ",
                )
                .bind(digest.as_str())
                .append(" and revoked_at is null and (expires_at is null or expires_at > ")
                .bind(now)
                .append(")"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_token)
            .transpose()
    }

    async fn revoke(
        &self,
        token: ApiToken,
        event: Option<DomainEventEnvelope>,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<ApiToken>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    if event.is_some() {
                        let previous_version =
                            token.aggregate_version.checked_sub(1).ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "revoked API token has no previous aggregate version".into(),
                                )
                            })?;
                        let rows = execute(
                            transaction,
                            sql_query::<()>("update api_tokens set revoked_at = ")
                                .bind(token.revoked_at)
                                .append(", aggregate_version = ")
                                .bind(token.aggregate_version)
                                .append(" where organization_id = ")
                                .bind(token.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(token.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(previous_version)
                                .append(" and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "API token changed while it was being revoked".into(),
                            )
                            .into());
                        }
                    } else {
                        let exists = fetch_optional::<i32, _>(
                            transaction,
                            sql_query::<i32>("select 1 from api_tokens where organization_id = ")
                                .bind(token.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(token.id.as_uuid()),
                        )
                        .await?
                        .is_some();
                        if !exists {
                            return Err(RepositoryError::NotFound.into());
                        }
                    }
                    if let Some(event) = &event {
                        store_outbox(transaction, event).await?;
                    }
                    store_idempotency(transaction, &idempotency, &token).await?;
                    Ok(IdempotentWrite {
                        value: token,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
