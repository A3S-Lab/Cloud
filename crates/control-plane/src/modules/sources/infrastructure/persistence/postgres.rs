use super::subscription_postgres::{
    map_row as map_subscription_row,
    select_columns_for_authoritative_fanout as authoritative_subscription_select_columns,
    GithubRepositorySubscriptionRow,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, AcceptSourceWebhook, BuildRecipe, ExternalSourceRevision, GitCommitSha,
    GitProvider, GitReference, GitRepository, GithubInstallationId, ISourceRevisionRepository,
    ISourceWebhookRepository, NewExternalSourceRevision, SourceRevisionAccepted,
    SourceWebhookAcceptance, SourceWebhookDelivery, WebhookDeliveryId, WebhookDeliveryReservation,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
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

struct SourceWebhookRow {
    provider: String,
    delivery_id: String,
    repository_url: String,
    repository_identity: String,
    installation_id: i64,
    branch_name: String,
    commit_sha: String,
    payload_digest: String,
    received_at: DateTime<Utc>,
}

impl FromRow for SourceWebhookRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            provider: decode(row, 0)?,
            delivery_id: decode(row, 1)?,
            repository_url: decode(row, 2)?,
            repository_identity: decode(row, 3)?,
            installation_id: decode(row, 4)?,
            branch_name: decode(row, 5)?,
            commit_sha: decode(row, 6)?,
            payload_digest: decode(row, 7)?,
            received_at: decode(row, 8)?,
        })
    }
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
impl ISourceWebhookRepository for PostgresSourceRevisionRepository {
    async fn accept_delivery(
        &self,
        request: AcceptSourceWebhook,
    ) -> Result<SourceWebhookAcceptance, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let delivery = request.delivery;
                    let branch_name = match &delivery.reference {
                        GitReference::Branch(value) => value,
                        _ => {
                            return Err(PostgresPersistenceError::Invariant(
                                "source webhook delivery is not a branch push".into(),
                            ))
                        }
                    };
                    let installation_id =
                        i64::try_from(delivery.installation_id.as_u64()).map_err(|_| {
                            PostgresPersistenceError::Invariant(
                                "source webhook installation ID exceeds PostgreSQL bigint".into(),
                            )
                        })?;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into source_webhook_inbox (provider, delivery_id, repository_url, repository_identity, installation_id, branch_name, commit_sha, payload_digest, received_at) values (",
                        )
                        .bind(delivery.provider.as_str())
                        .append(", ")
                        .bind(delivery.delivery_id.as_str())
                        .append(", ")
                        .bind(delivery.repository.canonical_url())
                        .append(", ")
                        .bind(delivery.repository.identity())
                        .append(", ")
                        .bind(installation_id)
                        .append(", ")
                        .bind(branch_name.as_str())
                        .append(", ")
                        .bind(delivery.commit_sha.as_str())
                        .append(", ")
                        .bind(delivery.payload_digest.as_str())
                        .append(", ")
                        .bind(delivery.received_at)
                        .append(") on conflict (provider, delivery_id) do nothing"),
                    )
                    .await?;
                    if inserted > 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "accepting source webhook delivery affected {inserted} rows"
                        )));
                    }
                    let row = fetch_optional::<SourceWebhookRow, _>(
                        transaction,
                        sql_query::<SourceWebhookRow>(
                            "select provider, delivery_id, repository_url, repository_identity, installation_id, branch_name, commit_sha, payload_digest, received_at from source_webhook_inbox where provider = ",
                        )
                        .bind(delivery.provider.as_str())
                        .append(" and delivery_id = ")
                        .bind(delivery.delivery_id.as_str())
                        .append(" for update"),
                    )
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "accepted source webhook delivery could not be read".into(),
                        )
                    })?;
                    let existing = map_webhook_row(row)?;
                    if !existing.same_payload_as(&delivery) {
                        return Err(RepositoryError::Conflict(
                            "webhook delivery ID was reused with another payload".into(),
                        )
                        .into());
                    }
                    if inserted == 0 {
                        return Ok(SourceWebhookAcceptance {
                            delivery: existing,
                            replayed: true,
                            revisions: Vec::new(),
                        });
                    }
                    let subscription_rows = match request.authoritative_connection_id {
                        Some(connection_id) => fetch_all::<GithubRepositorySubscriptionRow, _>(
                            transaction,
                            authoritative_subscription_select_columns()
                                .append(" where s.status = 'active' and c.status = 'active' and s.connection_id = ")
                                .bind(connection_id.as_uuid())
                                .append(" and s.installation_id = ")
                                .bind(installation_id)
                                .append(" and s.repository_provider = ")
                                .bind(delivery.provider.as_str())
                                .append(" and s.repository_identity = ")
                                .bind(delivery.repository.identity())
                                .append(" and s.branch_name = ")
                                .bind(branch_name.as_str())
                                .append(" order by s.organization_id asc, s.id asc for share of s, c"),
                        )
                        .await?,
                        None => Vec::new(),
                    };
                    let mut revisions = Vec::with_capacity(subscription_rows.len());
                    for row in subscription_rows {
                        let subscription = map_subscription_row(row)?;
                        let source_identity_digest =
                            delivery.repository.source_identity_digest(&delivery.commit_sha);
                        reserve_webhook_delivery(
                            transaction,
                            &WebhookDeliveryReservation {
                                organization_id: subscription.organization_id,
                                provider: delivery.provider,
                                delivery_id: delivery.delivery_id.clone(),
                                source_identity_digest,
                                received_at: delivery.received_at,
                            },
                        )
                        .await?;
                        let candidate = ExternalSourceRevision::accept(
                            NewExternalSourceRevision {
                                organization_id: subscription.organization_id,
                                project_id: subscription.project_id,
                                environment_id: subscription.environment_id,
                                id: SourceRevisionId::new(),
                                repository: delivery.repository.clone(),
                                commit_sha: delivery.commit_sha.clone(),
                                recipe: subscription.recipe,
                                accepted_at: delivery.received_at,
                            },
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "could not create source revision from subscription: {error}"
                            ))
                        })?;
                        let (revision, revision_inserted) =
                            insert_source_revision(transaction, &candidate).await?;
                        if revision_inserted {
                            let event = SourceRevisionAccepted::envelope(
                                &revision,
                                request.correlation_id,
                            )?;
                            store_outbox(transaction, &event).await?;
                        }
                        revisions.push(revision);
                    }
                    Ok(SourceWebhookAcceptance {
                        delivery: existing,
                        replayed: false,
                        revisions,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl ISourceRevisionRepository for PostgresSourceRevisionRepository {
    async fn find(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<ExternalSourceRevision, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<SourceRevisionRow>(
                    "select organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at from external_source_revisions where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(source_revision_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .ok_or(RepositoryError::NotFound)?;
        map_row(row).map_err(|error| match error {
            PostgresPersistenceError::Repository(error) => error,
            error => RepositoryError::Storage(error.to_string()),
        })
    }

    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ExternalSourceRevision>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let replay =
                        idempotency_replay::<ExternalSourceRevision>(transaction, &idempotency)
                            .await?;
                    replay
                        .map(|replay| {
                            replay.value.validate().map_err(|error| {
                                PostgresPersistenceError::Invariant(format!(
                                    "stored source idempotency response is invalid: {error}"
                                ))
                            })
                        })
                        .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }

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
                        reserve_webhook_delivery(transaction, delivery).await?;
                    }
                    let (revision, inserted) =
                        insert_source_revision(transaction, &request.revision).await?;
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

async fn reserve_webhook_delivery(
    transaction: &PostgresTransaction,
    delivery: &WebhookDeliveryReservation,
) -> Result<(), PostgresPersistenceError> {
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
        PostgresPersistenceError::Invariant("webhook delivery reservation disappeared".into())
    })?;
    if existing != delivery.source_identity_digest {
        return Err(RepositoryError::Conflict(
            "webhook delivery ID was reused for another source identity".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_source_revision(
    transaction: &PostgresTransaction,
    revision: &ExternalSourceRevision,
) -> Result<(ExternalSourceRevision, bool), PostgresPersistenceError> {
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into external_source_revisions (organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at) values (",
        )
        .bind(revision.organization_id.as_uuid())
        .append(", ")
        .bind(revision.project_id.as_uuid())
        .append(", ")
        .bind(revision.environment_id.as_uuid())
        .append(", ")
        .bind(revision.id.as_uuid())
        .append(", ")
        .bind(revision.repository.provider().as_str())
        .append(", ")
        .bind(revision.repository.canonical_url())
        .append(", ")
        .bind(revision.repository.identity())
        .append(", ")
        .bind(revision.commit_sha.as_str())
        .append(", ")
        .bind(serde_json::to_value(&revision.recipe)?)
        .append(", ")
        .bind(revision.recipe_digest.as_str())
        .append(", ")
        .bind(revision.aggregate_version)
        .append(", ")
        .bind(revision.accepted_at)
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
        source_revision_by_identity_query(revision),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("accepted source revision could not be read".into())
    })?;
    Ok((map_row(row)?, inserted))
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

fn map_webhook_row(
    row: SourceWebhookRow,
) -> Result<SourceWebhookDelivery, PostgresPersistenceError> {
    let provider = GitProvider::parse(&row.provider).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source webhook provider is invalid: {error}"
        ))
    })?;
    let delivery_id = WebhookDeliveryId::parse(row.delivery_id).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source webhook delivery ID is invalid: {error}"
        ))
    })?;
    let repository = GitRepository::parse(provider, &row.repository_url).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source webhook repository is invalid: {error}"
        ))
    })?;
    if repository.identity() != row.repository_identity {
        return Err(PostgresPersistenceError::Invariant(
            "stored source webhook repository identity does not match its URL".into(),
        ));
    }
    let installation_id = u64::try_from(row.installation_id)
        .ok()
        .and_then(|value| GithubInstallationId::parse(value).ok())
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "stored source webhook installation ID is invalid".into(),
            )
        })?;
    let reference = GitReference::parse("branch", row.branch_name).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source webhook branch is invalid: {error}"
        ))
    })?;
    let commit_sha = GitCommitSha::parse(row.commit_sha).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored source webhook commit is invalid: {error}"
        ))
    })?;
    SourceWebhookDelivery::restore(SourceWebhookDelivery {
        provider,
        delivery_id,
        repository,
        installation_id,
        reference,
        commit_sha,
        payload_digest: row.payload_digest,
        received_at: row.received_at,
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
