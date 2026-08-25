use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::developer_workflows::domain::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    IPullRequestPreviewPolicyRepository, PreviewPolicyRevisionWriteReference,
    MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
    PullRequestPreviewPolicyRevisionId, RepositoryError, SourceSubscriptionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_REVISIONS: &str = "select organization_id, project_id, source_environment_id, source_subscription_id, id, revision_number, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, owner_principal_id, lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, accepted_by, accepted_at from developer_pull_request_preview_policy_revisions";

#[derive(Clone)]
pub struct PostgresPullRequestPreviewPolicyRepository {
    executor: PostgresExecutor,
}

impl PostgresPullRequestPreviewPolicyRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IPullRequestPreviewPolicyRepository for PostgresPullRequestPreviewPolicyRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<PreviewPolicyRevisionWriteReference>(
                            transaction,
                            &idempotency,
                        )
                        .await?
                    else {
                        return Ok(None);
                    };
                    load_reference(transaction, reference.value).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn accept(
        &self,
        write: AcceptPullRequestPreviewPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    transaction
                        .advisory_xact_lock(
                            "a3s.cloud.developer-preview-policy",
                            &format!(
                                "{}:{}",
                                write.revision.organization_id,
                                write.revision.source_subscription_id
                            ),
                        )
                        .await?;
                    if let Some(reference) =
                        idempotency_replay::<PreviewPolicyRevisionWriteReference>(
                            transaction,
                            &write.idempotency,
                        )
                        .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_reference(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }

                    let current = load_current(
                        transaction,
                        write.revision.organization_id,
                        write.revision.project_id,
                        write.revision.source_environment_id,
                        write.revision.source_subscription_id,
                    )
                    .await?;
                    if let Some(existing) = current.as_ref() {
                        ensure_same_policy(existing, &write.revision)?;
                        if existing.contract == write.revision.contract {
                            let reference = PreviewPolicyRevisionWriteReference::from(existing);
                            store_idempotency(transaction, &write.idempotency, &reference).await?;
                            return Ok(IdempotentWrite {
                                value: existing.clone(),
                                replayed: true,
                            });
                        }
                    }
                    let actual_previous = current.as_ref().map(|revision| revision.id);
                    let expected_number = current
                        .as_ref()
                        .map_or(Some(1), |revision| revision.revision_number.checked_add(1))
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "Preview policy revision number overflowed".into(),
                            )
                        })?;
                    if actual_previous != write.expected_previous_revision_id
                        || write.revision.revision_number != expected_number
                    {
                        return Err(RepositoryError::Conflict(
                            "Preview policy head advanced before acceptance".into(),
                        )
                        .into());
                    }

                    let inserted = match insert_revision(transaction, &write.revision).await {
                        Ok(rows) => rows,
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    if inserted != 1 {
                        return Err(RepositoryError::Conflict(
                            "Preview policy revision identity is already in use".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: write.revision.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "developer.pull-request-preview-policy.revision-accepted",
                            aggregate_id: write.revision.source_subscription_id.as_uuid(),
                            occurred_at: write.revision.accepted_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                write.revision.project_id,
                                Some(write.revision.source_environment_id),
                            ),
                            details: audit_details(&write.revision),
                        },
                    )
                    .await?;
                    let reference = PreviewPolicyRevisionWriteReference::from(&write.revision);
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.revision,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        revision_id: PullRequestPreviewPolicyRevisionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and source_environment_id = ")
                    .bind(source_environment_id.as_uuid())
                    .append(" and source_subscription_id = ")
                    .bind(source_subscription_id.as_uuid())
                    .append(" and id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
            .map_err(storage)
    }

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(current_query(
                organization_id,
                project_id,
                source_environment_id,
                source_subscription_id,
            ))
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
            .map_err(storage)
    }

    async fn find_effective_at(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        fact_occurred_at: DateTime<Utc>,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and source_environment_id = ")
                    .bind(source_environment_id.as_uuid())
                    .append(" and source_subscription_id = ")
                    .bind(source_subscription_id.as_uuid())
                    .append(" and accepted_at <= ")
                    .bind(fact_occurred_at)
                    .append(" order by accepted_at desc, revision_number desc limit 1"),
            )
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
            .map_err(storage)
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        limit: usize,
    ) -> Result<Vec<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                revision_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and source_environment_id = ")
                    .bind(source_environment_id.as_uuid())
                    .append(" and source_subscription_id = ")
                    .bind(source_subscription_id.as_uuid())
                    .append(" order by revision_number asc limit ")
                    .bind(limit.min(MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE)),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(|row| decode_revision(row).map_err(storage))
            .collect()
    }
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    revision: &AcceptedPullRequestPreviewPolicyRevision,
) -> Result<u64, PostgresPersistenceError> {
    let policy = revision.contract.policy();
    execute(
        transaction,
        sql_query::<()>("insert into developer_pull_request_preview_policy_revisions (organization_id, project_id, source_environment_id, source_subscription_id, id, revision_number, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, owner_principal_id, lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, accepted_by, accepted_at) values (")
            .bind(revision.organization_id.as_uuid())
            .append(", ")
            .bind(revision.project_id.as_uuid())
            .append(", ")
            .bind(revision.source_environment_id.as_uuid())
            .append(", ")
            .bind(revision.source_subscription_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(policy.installation_id.as_u64())
            .append(", ")
            .bind(policy.base_repository.provider().as_str())
            .append(", ")
            .bind(policy.base_repository.canonical_url())
            .append(", ")
            .bind(policy.base_repository.identity())
            .append(", ")
            .bind(policy.base_branch.as_str())
            .append(", ")
            .bind(revision.contract.schema())
            .append(", ")
            .bind(revision.contract.canonical_acl())
            .append(", ")
            .bind(revision.contract.digest().as_str())
            .append(", ")
            .bind(policy.owner_principal_id.as_uuid())
            .append(", ")
            .bind(u64::from(policy.lifetime_seconds))
            .append(", ")
            .bind(u64::from(policy.maximum_active_previews))
            .append(", ")
            .bind(policy.fork_policy.as_str())
            .append(", ")
            .bind(policy.allow_protected_secrets_for_trusted_sources)
            .append(", ")
            .bind(u64::from(policy.quota.maximum_workloads))
            .append(", ")
            .bind(policy.quota.cpu_millis)
            .append(", ")
            .bind(policy.quota.memory_bytes)
            .append(", ")
            .bind(policy.quota.ephemeral_storage_bytes)
            .append(", ")
            .bind(revision.accepted_by.as_uuid())
            .append(", ")
            .bind(revision.accepted_at)
            .append(") on conflict do nothing"),
    )
    .await
}

pub(super) async fn load_reference(
    transaction: &PostgresTransaction,
    reference: PreviewPolicyRevisionWriteReference,
) -> Result<AcceptedPullRequestPreviewPolicyRevision, PostgresPersistenceError> {
    fetch_optional::<PreviewPolicyRevisionRow, _>(
        transaction,
        revision_query()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and source_environment_id = ")
            .bind(reference.source_environment_id.as_uuid())
            .append(" and source_subscription_id = ")
            .bind(reference.source_subscription_id.as_uuid())
            .append(" and id = ")
            .bind(reference.preview_policy_revision_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Preview policy idempotency points to a missing revision".into(),
        )
    })
    .and_then(decode_revision)
}

async fn load_current(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, PostgresPersistenceError> {
    fetch_optional::<PreviewPolicyRevisionRow, _>(
        transaction,
        current_query(
            organization_id,
            project_id,
            source_environment_id,
            source_subscription_id,
        ),
    )
    .await?
    .map(decode_revision)
    .transpose()
}

fn current_query(
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
) -> SqlQuery<PreviewPolicyRevisionRow> {
    revision_query()
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(project_id.as_uuid())
        .append(" and source_environment_id = ")
        .bind(source_environment_id.as_uuid())
        .append(" and source_subscription_id = ")
        .bind(source_subscription_id.as_uuid())
        .append(" order by revision_number desc limit 1")
}

fn ensure_same_policy(
    existing: &AcceptedPullRequestPreviewPolicyRevision,
    candidate: &AcceptedPullRequestPreviewPolicyRevision,
) -> Result<(), PostgresPersistenceError> {
    let existing_policy = existing.contract.policy();
    let candidate_policy = candidate.contract.policy();
    if existing.organization_id != candidate.organization_id
        || existing.project_id != candidate.project_id
        || existing.source_environment_id != candidate.source_environment_id
        || existing.source_subscription_id != candidate.source_subscription_id
        || existing_policy.installation_id != candidate_policy.installation_id
        || existing_policy.base_repository != candidate_policy.base_repository
        || existing_policy.base_branch != candidate_policy.base_branch
    {
        return Err(RepositoryError::Conflict(
            "Preview policy identity collided with another source binding".into(),
        )
        .into());
    }
    Ok(())
}

fn audit_details(revision: &AcceptedPullRequestPreviewPolicyRevision) -> serde_json::Value {
    let policy = revision.contract.policy();
    serde_json::json!({
        "sourceSubscriptionId": revision.source_subscription_id,
        "previewPolicyRevisionId": revision.id,
        "revisionNumber": revision.revision_number,
        "policyDigest": revision.contract.digest(),
        "installationId": policy.installation_id.as_u64(),
        "baseRepositoryIdentity": policy.base_repository.identity(),
        "baseBranch": policy.base_branch.as_str(),
        "ownerPrincipalId": policy.owner_principal_id,
        "forkPolicy": policy.fork_policy.as_str(),
    })
}

fn revision_query() -> SqlQuery<PreviewPolicyRevisionRow> {
    sql_query::<PreviewPolicyRevisionRow>(SELECT_REVISIONS)
}

struct PreviewPolicyRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    source_environment_id: Uuid,
    source_subscription_id: Uuid,
    id: Uuid,
    revision_number: u64,
    installation_id: u64,
    repository_provider: String,
    repository_url: String,
    repository_identity: String,
    base_branch: String,
    policy_schema: String,
    canonical_acl: String,
    policy_digest: String,
    owner_principal_id: Uuid,
    lifetime_seconds: u64,
    maximum_active_previews: u64,
    fork_policy: String,
    allow_protected_secrets_for_trusted_sources: bool,
    maximum_workloads: u64,
    cpu_millis: u64,
    memory_bytes: u64,
    ephemeral_storage_bytes: u64,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for PreviewPolicyRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            source_environment_id: decode(row, 2)?,
            source_subscription_id: decode(row, 3)?,
            id: decode(row, 4)?,
            revision_number: decode(row, 5)?,
            installation_id: decode(row, 6)?,
            repository_provider: decode(row, 7)?,
            repository_url: decode(row, 8)?,
            repository_identity: decode(row, 9)?,
            base_branch: decode(row, 10)?,
            policy_schema: decode(row, 11)?,
            canonical_acl: decode(row, 12)?,
            policy_digest: decode(row, 13)?,
            owner_principal_id: decode(row, 14)?,
            lifetime_seconds: decode(row, 15)?,
            maximum_active_previews: decode(row, 16)?,
            fork_policy: decode(row, 17)?,
            allow_protected_secrets_for_trusted_sources: decode(row, 18)?,
            maximum_workloads: decode(row, 19)?,
            cpu_millis: decode(row, 20)?,
            memory_bytes: decode(row, 21)?,
            ephemeral_storage_bytes: decode(row, 22)?,
            accepted_by: decode(row, 23)?,
            accepted_at: decode(row, 24)?,
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

fn decode_revision(
    row: PreviewPolicyRevisionRow,
) -> Result<AcceptedPullRequestPreviewPolicyRevision, PostgresPersistenceError> {
    let revision = AcceptedPullRequestPreviewPolicyRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.source_environment_id),
        SourceSubscriptionId::from_uuid(row.source_subscription_id),
        PullRequestPreviewPolicyRevisionId::from_uuid(row.id),
        row.revision_number,
        &row.canonical_acl,
        &row.policy_digest,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored Preview policy revision is invalid: {error}"
        ))
    })?;
    let policy = revision.contract.policy();
    if row.installation_id != policy.installation_id.as_u64()
        || row.repository_provider != policy.base_repository.provider().as_str()
        || row.repository_url != policy.base_repository.canonical_url()
        || row.repository_identity != policy.base_repository.identity()
        || row.base_branch != policy.base_branch.as_str()
        || row.policy_schema != revision.contract.schema()
        || row.owner_principal_id != policy.owner_principal_id.as_uuid()
        || row.lifetime_seconds != u64::from(policy.lifetime_seconds)
        || row.maximum_active_previews != u64::from(policy.maximum_active_previews)
        || row.fork_policy != policy.fork_policy.as_str()
        || row.allow_protected_secrets_for_trusted_sources
            != policy.allow_protected_secrets_for_trusted_sources
        || row.maximum_workloads != u64::from(policy.quota.maximum_workloads)
        || row.cpu_millis != policy.quota.cpu_millis
        || row.memory_bytes != policy.quota.memory_bytes
        || row.ephemeral_storage_bytes != policy.quota.ephemeral_storage_bytes
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored Preview policy columns drifted from canonical ACL".into(),
        ));
    }
    Ok(revision)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(format!(
        "could not query pull-request Preview policy revisions: {error}"
    ))
}

#[cfg(test)]
mod migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/153_developer_pull_request_preview_policy_revisions.sql"
    ));

    #[test]
    fn migration_keeps_policy_revisions_append_only_and_inside_owner_boundaries() {
        for expected in [
            "create table developer_pull_request_preview_policy_revisions",
            "a3s.cloud.pull-request-preview-policy.v1",
            "references github_repository_subscriptions",
            "developer_preview_policy_owner_membership_fk",
            "developer_preview_policy_actor_membership_fk",
            "validate_developer_preview_policy_revision",
            "does not match its exact active source subscription",
            "Preview policy revision sequence is not monotonic",
            "developer_preview_policy_revisions_immutable",
            "accepted Preview policy revisions are immutable",
            "no Environment, SourceRevision, BuildRun, Workload, Route, Operation, timer, scheduler, webhook, or credential authority",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "missing migration guard {expected}"
            );
        }
        for forbidden in [
            "create table environments",
            "create table external_source_revisions",
            "create table build_runs",
            "create table workloads",
            "create table routes",
            "create table operations",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration crossed owning context with {forbidden}"
            );
        }
    }
}
