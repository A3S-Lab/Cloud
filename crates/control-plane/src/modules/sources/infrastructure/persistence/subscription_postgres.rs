use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, store_idempotency,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError, SourceConnectionId,
    SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, CreateGithubRepositorySubscription, DeactivateGithubRepositorySubscription,
    GitProvider, GitReference, GitRepository, GithubInstallationId, GithubRepositorySubscription,
    GithubRepositorySubscriptionStatus, ISourceSubscriptionRepository,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

pub(super) struct GithubRepositorySubscriptionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    connection_id: Uuid,
    installation_id: i64,
    repository_provider: String,
    repository_url: String,
    repository_identity: String,
    branch_name: String,
    recipe: Value,
    recipe_digest: String,
    status: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    deactivated_at: Option<DateTime<Utc>>,
}

impl FromRow for GithubRepositorySubscriptionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            connection_id: decode(row, 4)?,
            installation_id: decode(row, 5)?,
            repository_provider: decode(row, 6)?,
            repository_url: decode(row, 7)?,
            repository_identity: decode(row, 8)?,
            branch_name: decode(row, 9)?,
            recipe: decode(row, 10)?,
            recipe_digest: decode(row, 11)?,
            status: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
            created_at: decode(row, 14)?,
            deactivated_at: decode(row, 15)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresSourceSubscriptionRepository {
    executor: PostgresExecutor,
}

impl PostgresSourceSubscriptionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl ISourceSubscriptionRepository for PostgresSourceSubscriptionRepository {
    async fn create(
        &self,
        request: CreateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<GithubRepositorySubscription>(
                        transaction,
                        &request.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: validate_idempotency(replayed.value)?,
                            replayed: true,
                        });
                    }
                    let installation_id = as_i64(request.subscription.installation_id)?;
                    if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from github_source_connections where organization_id = ",
                        )
                        .bind(request.subscription.organization_id.as_uuid())
                        .append(" and id = ")
                        .bind(request.subscription.connection_id.as_uuid())
                        .append(" and installation_id = ")
                        .bind(installation_id)
                        .append(" and status = 'active' for share"),
                    )
                    .await?
                    .is_none()
                    {
                        return Err(RepositoryError::Conflict(
                            "GitHub source connection is not active".into(),
                        )
                        .into());
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into github_repository_subscriptions (organization_id, project_id, environment_id, id, connection_id, installation_id, repository_provider, repository_url, repository_identity, branch_name, recipe, recipe_digest, status, aggregate_version, created_at, deactivated_at) values (",
                        )
                        .bind(request.subscription.organization_id.as_uuid())
                        .append(", ")
                        .bind(request.subscription.project_id.as_uuid())
                        .append(", ")
                        .bind(request.subscription.environment_id.as_uuid())
                        .append(", ")
                        .bind(request.subscription.id.as_uuid())
                        .append(", ")
                        .bind(request.subscription.connection_id.as_uuid())
                        .append(", ")
                        .bind(installation_id)
                        .append(", ")
                        .bind(request.subscription.repository.provider().as_str())
                        .append(", ")
                        .bind(request.subscription.repository.canonical_url())
                        .append(", ")
                        .bind(request.subscription.repository.identity())
                        .append(", ")
                        .bind(request.subscription.branch_name())
                        .append(", ")
                        .bind(serde_json::to_value(&request.subscription.recipe)?)
                        .append(", ")
                        .bind(request.subscription.recipe_digest.as_str())
                        .append(", ")
                        .bind(request.subscription.status.as_str())
                        .append(", ")
                        .bind(request.subscription.aggregate_version)
                        .append(", ")
                        .bind(request.subscription.created_at)
                        .append(", ")
                        .bind(request.subscription.deactivated_at)
                        .append(") on conflict do nothing"),
                    )
                    .await;
                    let inserted = match inserted {
                        Ok(rows @ 0..=1) => rows == 1,
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating GitHub repository subscription affected {rows} rows"
                            )))
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    let row = fetch_optional::<GithubRepositorySubscriptionRow, _>(
                        transaction,
                        active_identity_query(&request.subscription).append(" for update"),
                    )
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "created subscription could not be read by active identity".into(),
                        )
                    })?;
                    let subscription = map_row(row)?;
                    if inserted {
                        store_outbox(transaction, &request.event).await?;
                    }
                    store_idempotency(transaction, &request.idempotency, &subscription).await?;
                    Ok(IdempotentWrite {
                        value: subscription,
                        replayed: !inserted,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
    ) -> Result<Option<GithubRepositorySubscription>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                select_columns()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(subscription_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
            .map_err(persistence_error)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GithubRepositorySubscription>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_columns()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .map(|result| result.map_err(persistence_error))
            .collect()
    }

    async fn deactivate(
        &self,
        request: DeactivateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<GithubRepositorySubscription>(
                        transaction,
                        &request.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: validate_idempotency(replayed.value)?,
                            replayed: true,
                        });
                    }
                    let row = fetch_optional::<GithubRepositorySubscriptionRow, _>(
                        transaction,
                        select_columns()
                            .append(" where organization_id = ")
                            .bind(request.subscription.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(request.subscription.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let existing = map_row(row)?;
                    if existing == request.subscription {
                        store_idempotency(transaction, &request.idempotency, &existing).await?;
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    require_deactivation_identity(&existing, &request)?;
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update github_repository_subscriptions set status = ")
                            .bind(request.subscription.status.as_str())
                            .append(", aggregate_version = ")
                            .bind(request.subscription.aggregate_version)
                            .append(", deactivated_at = ")
                            .bind(request.subscription.deactivated_at)
                            .append(" where organization_id = ")
                            .bind(request.subscription.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(request.subscription.id.as_uuid())
                            .append(" and status = 'active' and aggregate_version = ")
                            .bind(request.previous_version),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "GitHub repository subscription changed concurrently".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &request.event).await?;
                    store_idempotency(transaction, &request.idempotency, &request.subscription)
                        .await?;
                    Ok(IdempotentWrite {
                        value: request.subscription,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn require_deactivation_identity(
    existing: &GithubRepositorySubscription,
    request: &DeactivateGithubRepositorySubscription,
) -> Result<(), PostgresPersistenceError> {
    let target = &request.subscription;
    if existing.aggregate_version != request.previous_version
        || existing.organization_id != target.organization_id
        || existing.project_id != target.project_id
        || existing.environment_id != target.environment_id
        || existing.connection_id != target.connection_id
        || existing.installation_id != target.installation_id
        || existing.repository != target.repository
        || existing.branch != target.branch
        || existing.recipe != target.recipe
        || target.status != GithubRepositorySubscriptionStatus::Inactive
        || target.aggregate_version != request.previous_version + 1
    {
        return Err(RepositoryError::Conflict(
            "GitHub repository subscription changed concurrently".into(),
        )
        .into());
    }
    Ok(())
}

fn active_identity_query(
    subscription: &GithubRepositorySubscription,
) -> a3s_orm::SqlQuery<GithubRepositorySubscriptionRow> {
    select_columns()
        .append(" where organization_id = ")
        .bind(subscription.organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(subscription.project_id.as_uuid())
        .append(" and environment_id = ")
        .bind(subscription.environment_id.as_uuid())
        .append(" and connection_id = ")
        .bind(subscription.connection_id.as_uuid())
        .append(" and repository_identity = ")
        .bind(subscription.repository.identity())
        .append(" and branch_name = ")
        .bind(subscription.branch_name())
        .append(" and recipe_digest = ")
        .bind(subscription.recipe_digest.as_str())
        .append(" and status = 'active'")
}

pub(super) fn select_columns() -> a3s_orm::SqlQuery<GithubRepositorySubscriptionRow> {
    sql_query::<GithubRepositorySubscriptionRow>(
        "select organization_id, project_id, environment_id, id, connection_id, installation_id, repository_provider, repository_url, repository_identity, branch_name, recipe, recipe_digest, status, aggregate_version, created_at, deactivated_at from github_repository_subscriptions",
    )
}

pub(super) fn select_columns_for_authoritative_fanout(
) -> a3s_orm::SqlQuery<GithubRepositorySubscriptionRow> {
    sql_query::<GithubRepositorySubscriptionRow>(
        "select s.organization_id, s.project_id, s.environment_id, s.id, s.connection_id, s.installation_id, s.repository_provider, s.repository_url, s.repository_identity, s.branch_name, s.recipe, s.recipe_digest, s.status, s.aggregate_version, s.created_at, s.deactivated_at from github_repository_subscriptions s join github_source_connections c on c.organization_id = s.organization_id and c.id = s.connection_id and c.installation_id = s.installation_id",
    )
}

pub(super) fn map_row(
    row: GithubRepositorySubscriptionRow,
) -> Result<GithubRepositorySubscription, PostgresPersistenceError> {
    let provider = GitProvider::parse(&row.repository_provider).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored subscription repository provider is invalid: {error}"
        ))
    })?;
    let repository = GitRepository::parse(provider, &row.repository_url).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored subscription repository URL is invalid: {error}"
        ))
    })?;
    if repository.identity() != row.repository_identity {
        return Err(PostgresPersistenceError::Invariant(
            "stored subscription repository identity does not match its URL".into(),
        ));
    }
    let installation_id = u64::try_from(row.installation_id)
        .map_err(|_| {
            PostgresPersistenceError::Invariant(
                "stored subscription installation ID is negative".into(),
            )
        })
        .and_then(|value| {
            GithubInstallationId::parse(value).map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored subscription installation ID is invalid: {error}"
                ))
            })
        })?;
    let branch = GitReference::parse("branch", row.branch_name).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored subscription branch is invalid: {error}"
        ))
    })?;
    let recipe = serde_json::from_value::<BuildRecipe>(row.recipe)
        .map_err(PostgresPersistenceError::Serialization)?
        .validate()
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored subscription recipe is invalid: {error}"
            ))
        })?;
    let status = GithubRepositorySubscriptionStatus::parse(&row.status)
        .map_err(PostgresPersistenceError::Invariant)?;
    GithubRepositorySubscription::restore(GithubRepositorySubscription {
        id: SourceSubscriptionId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        connection_id: SourceConnectionId::from_uuid(row.connection_id),
        installation_id,
        repository,
        branch,
        recipe,
        recipe_digest: row.recipe_digest,
        status,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        deactivated_at: row.deactivated_at,
    })
    .map_err(PostgresPersistenceError::Invariant)
}

fn validate_idempotency(
    subscription: GithubRepositorySubscription,
) -> Result<GithubRepositorySubscription, PostgresPersistenceError> {
    GithubRepositorySubscription::restore(subscription).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored subscription idempotency response is invalid: {error}"
        ))
    })
}

fn as_i64(value: GithubInstallationId) -> Result<i64, PostgresPersistenceError> {
    i64::try_from(value.as_u64()).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "GitHub installation ID exceeds PostgreSQL bigint".into(),
        )
    })
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn persistence_error(error: PostgresPersistenceError) -> RepositoryError {
    match error {
        PostgresPersistenceError::Repository(error) => error,
        error => RepositoryError::Storage(error.to_string()),
    }
}
