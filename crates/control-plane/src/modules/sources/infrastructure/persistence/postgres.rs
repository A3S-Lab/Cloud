use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, store_idempotency,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider,
    GitRepository, ISourceRevisionRepository,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

struct SourceRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    repository_provider: String,
    repository_url: String,
    repository_identity: String,
    commit_sha: String,
    recipe: Value,
    recipe_digest: String,
    aggregate_version: u64,
    accepted_at: DateTime<Utc>,
}

impl FromRow for SourceRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            repository_provider: decode(row, 4)?,
            repository_url: decode(row, 5)?,
            repository_identity: decode(row, 6)?,
            commit_sha: decode(row, 7)?,
            recipe: decode(row, 8)?,
            recipe_digest: decode(row, 9)?,
            aggregate_version: decode(row, 10)?,
            accepted_at: decode(row, 11)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresSourceRevisionRepository {
    executor: PostgresExecutor,
}

impl PostgresSourceRevisionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl ISourceRevisionRepository for PostgresSourceRevisionRepository {
    async fn accept(
        &self,
        request: AcceptSourceRevision,
    ) -> Result<IdempotentWrite<ExternalSourceRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<ExternalSourceRevision>(
                        transaction,
                        &request.idempotency,
                    )
                    .await?
                    {
                        let revision = replayed.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "stored source idempotency response is invalid: {error}"
                            ))
                        })?;
                        return Ok(IdempotentWrite {
                            value: revision,
                            replayed: true,
                        });
                    }
                    if let Some(delivery) = &request.webhook_delivery {
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into source_webhook_deliveries (organization_id, provider, delivery_id, source_identity_digest, received_at) values (",
                            )
                            .bind(delivery.organization_id.as_uuid())
                            .append(", ")
                            .bind(delivery.provider.as_str())
                            .append(", ")
                            .bind(delivery.delivery_id.as_str())
                            .append(", ")
                            .bind(delivery.source_identity_digest.as_str())
                            .append(", ")
                            .bind(delivery.received_at)
                            .append(") on conflict (organization_id, provider, delivery_id) do nothing"),
                        )
                        .await?;
                        let existing = fetch_optional::<String, _>(
                            transaction,
                            sql_query::<String>(
                                "select source_identity_digest from source_webhook_deliveries where organization_id = ",
                            )
                            .bind(delivery.organization_id.as_uuid())
                            .append(" and provider = ")
                            .bind(delivery.provider.as_str())
                            .append(" and delivery_id = ")
                            .bind(delivery.delivery_id.as_str())
                            .append(" for update"),
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "webhook delivery reservation disappeared".into(),
                            )
                        })?;
                        if existing != delivery.source_identity_digest {
                            return Err(RepositoryError::Conflict(
                                "webhook delivery ID was reused for another source identity".into(),
                            )
                            .into());
                        }
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into external_source_revisions (organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at) values (",
                        )
                        .bind(request.revision.organization_id.as_uuid())
                        .append(", ")
                        .bind(request.revision.project_id.as_uuid())
                        .append(", ")
                        .bind(request.revision.environment_id.as_uuid())
                        .append(", ")
                        .bind(request.revision.id.as_uuid())
                        .append(", ")
                        .bind(request.revision.repository.provider().as_str())
                        .append(", ")
                        .bind(request.revision.repository.canonical_url())
                        .append(", ")
                        .bind(request.revision.repository.identity())
                        .append(", ")
                        .bind(request.revision.commit_sha.as_str())
                        .append(", ")
                        .bind(serde_json::to_value(&request.revision.recipe)?)
                        .append(", ")
                        .bind(request.revision.recipe_digest.as_str())
                        .append(", ")
                        .bind(request.revision.aggregate_version)
                        .append(", ")
                        .bind(request.revision.accepted_at)
                        .append(
                            ") on conflict (organization_id, project_id, environment_id, repository_identity, commit_sha, recipe_digest) do nothing",
                        ),
                    )
                    .await;
                    let inserted = match inserted {
                        Ok(rows @ 0..=1) => rows == 1,
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "accepting source revision affected {rows} rows"
                            )))
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    let row = fetch_optional::<SourceRevisionRow, _>(
                        transaction,
                        source_revision_by_identity_query(&request.revision),
                    )
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "accepted source revision could not be read".into(),
                        )
                    })?;
                    let revision = map_row(row)?;
                    if inserted {
                        store_outbox(transaction, &request.event).await?;
                    }
                    store_idempotency(transaction, &request.idempotency, &revision).await?;
                    Ok(IdempotentWrite {
                        value: revision,
                        replayed: !inserted,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<ExternalSourceRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<SourceRevisionRow>(
                    "select organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at from external_source_revisions where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by accepted_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .map(|result| {
                result.map_err(|error| match error {
                    PostgresPersistenceError::Repository(error) => error,
                    error => RepositoryError::Storage(error.to_string()),
                })
            })
            .collect()
    }
}

fn source_revision_by_identity_query(
    revision: &ExternalSourceRevision,
) -> a3s_orm::SqlQuery<SourceRevisionRow> {
    sql_query::<SourceRevisionRow>(
        "select organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at from external_source_revisions where organization_id = ",
    )
    .bind(revision.organization_id.as_uuid())
    .append(" and project_id = ")
    .bind(revision.project_id.as_uuid())
    .append(" and environment_id = ")
    .bind(revision.environment_id.as_uuid())
    .append(" and repository_identity = ")
    .bind(revision.repository.identity())
    .append(" and commit_sha = ")
    .bind(revision.commit_sha.as_str())
    .append(" and recipe_digest = ")
    .bind(revision.recipe_digest.as_str())
}

fn map_row(row: SourceRevisionRow) -> Result<ExternalSourceRevision, PostgresPersistenceError> {
    let SourceRevisionRow {
        organization_id,
        project_id,
        environment_id,
        id,
        repository_provider,
        repository_url,
        repository_identity,
        commit_sha,
        recipe,
        recipe_digest,
        aggregate_version,
        accepted_at,
    } = row;
    let provider = GitProvider::parse(&repository_provider).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source repository provider is invalid: {error}"
        ))
    })?;
    let repository = GitRepository::parse(provider, &repository_url).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source repository URL is invalid: {error}"
        ))
    })?;
    if repository.identity() != repository_identity {
        return Err(PostgresPersistenceError::Invariant(
            "stored source repository identity does not match its URL".into(),
        ));
    }
    let commit_sha = GitCommitSha::parse(commit_sha).map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored Git commit SHA is invalid: {error}"))
    })?;
    let recipe = serde_json::from_value::<BuildRecipe>(recipe)
        .map_err(PostgresPersistenceError::Serialization)?
        .validate()
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored source build recipe is invalid: {error}"
            ))
        })?;
    ExternalSourceRevision::restore(ExternalSourceRevision {
        organization_id: OrganizationId::from_uuid(organization_id),
        project_id: ProjectId::from_uuid(project_id),
        environment_id: EnvironmentId::from_uuid(environment_id),
        id: SourceRevisionId::from_uuid(id),
        repository,
        commit_sha,
        recipe,
        recipe_digest,
        aggregate_version,
        accepted_at,
    })
    .map_err(PostgresPersistenceError::Invariant)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
