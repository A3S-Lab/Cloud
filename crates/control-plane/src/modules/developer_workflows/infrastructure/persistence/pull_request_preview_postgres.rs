use super::preview_policy_postgres::load_reference;
use crate::infrastructure::{execute, fetch_optional, transaction_error, PostgresPersistenceError};
use crate::modules::developer_workflows::domain::{
    CommitPullRequestPreviewProjection, IPullRequestPreviewProjectionRepository,
    PreviewCleanupReason, PreviewPolicyRevisionWriteReference, PullRequestChangeKind,
    PullRequestPreview, PullRequestPreviewProjectionOutcome, PullRequestPreviewProjectionReceipt,
    PullRequestPreviewStatus, PullRequestPreviewVersion,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, IdempotentWrite, OrganizationId, ProjectId, PullRequestPreviewId,
    PullRequestPreviewPolicyRevisionId, RepositoryError, Sha256Digest, SourcePullRequestChangeId,
    SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_PREVIEWS: &str = "select organization_id, project_id, source_environment_id, source_subscription_id, id, environment_id, policy_revision_id, pull_request_id, pull_request_number, head_repository_provider, head_repository_url, head_repository_identity, head_branch, head_commit_sha, provider_created_at, last_provider_updated_at, last_change_kind, last_merged, expires_at, status, cleanup_reason, cleanup_requested_at, aggregate_version from developer_pull_request_previews";
const SELECT_RECEIPTS: &str = "select organization_id, source_pull_request_change_id, project_id, source_environment_id, source_subscription_id, pull_request_id, pull_request_number, fact_digest, fact_occurred_at, policy_revision_id, preview_id, preview_aggregate_version, outcome from developer_pull_request_change_projections";

#[derive(Clone)]
pub struct PostgresPullRequestPreviewProjectionRepository {
    executor: PostgresExecutor,
}

impl PostgresPullRequestPreviewProjectionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IPullRequestPreviewProjectionRepository for PostgresPullRequestPreviewProjectionRepository {
    async fn find_receipt(
        &self,
        organization_id: OrganizationId,
        source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> Result<Option<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_receipt(transaction, organization_id, source_pull_request_change_id).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_preview(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        pull_request_id: u64,
    ) -> Result<Option<PullRequestPreview>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_preview(
                        transaction,
                        organization_id,
                        project_id,
                        source_environment_id,
                        source_subscription_id,
                        pull_request_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn commit_projection(
        &self,
        write: CommitPullRequestPreviewProjection,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    transaction
                        .advisory_xact_lock(
                            "a3s.cloud.developer-pull-request-preview",
                            &format!(
                                "{}:{}:{}",
                                write.receipt.organization_id,
                                write.receipt.source_subscription_id,
                                write.receipt.pull_request_id
                            ),
                        )
                        .await?;
                    if let Some(existing) = load_receipt(
                        transaction,
                        write.receipt.organization_id,
                        write.receipt.source_pull_request_change_id,
                    )
                    .await?
                    {
                        return exact_replay(existing, &write.receipt);
                    }

                    let current = load_preview(
                        transaction,
                        write.receipt.organization_id,
                        write.receipt.project_id,
                        write.receipt.source_environment_id,
                        write.receipt.source_subscription_id,
                        write.receipt.pull_request_id,
                    )
                    .await?;
                    let observed = current.as_ref().map(|preview| PullRequestPreviewVersion {
                        id: preview.id,
                        aggregate_version: preview.aggregate_version,
                    });
                    if observed != write.expected_preview {
                        return Err(conflict(
                            "pull-request Preview advanced before projection commit",
                        ));
                    }
                    if let Some(preview) = &write.preview {
                        store_preview(transaction, preview, write.expected_preview).await?;
                    }
                    let inserted = insert_receipt(transaction, &write.receipt).await?;
                    if inserted != 1 {
                        let existing = load_receipt(
                            transaction,
                            write.receipt.organization_id,
                            write.receipt.source_pull_request_change_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "conflicting Preview receipt could not be reloaded".into(),
                            )
                        })?;
                        return exact_replay(existing, &write.receipt);
                    }
                    Ok(IdempotentWrite {
                        value: write.receipt,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn load_receipt(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    source_pull_request_change_id: SourcePullRequestChangeId,
) -> Result<Option<PullRequestPreviewProjectionReceipt>, PostgresPersistenceError> {
    fetch_optional::<PullRequestPreviewProjectionReceiptRow, _>(
        transaction,
        sql_query::<PullRequestPreviewProjectionReceiptRow>(SELECT_RECEIPTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and source_pull_request_change_id = ")
            .bind(source_pull_request_change_id.as_uuid()),
    )
    .await?
    .map(map_receipt)
    .transpose()
}

#[allow(clippy::too_many_arguments)]
async fn load_preview(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    pull_request_id: u64,
) -> Result<Option<PullRequestPreview>, PostgresPersistenceError> {
    let Some(row) = fetch_optional::<PullRequestPreviewRow, _>(
        transaction,
        sql_query::<PullRequestPreviewRow>(SELECT_PREVIEWS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and source_environment_id = ")
            .bind(source_environment_id.as_uuid())
            .append(" and source_subscription_id = ")
            .bind(source_subscription_id.as_uuid())
            .append(" and pull_request_id = ")
            .bind(pull_request_id),
    )
    .await?
    else {
        return Ok(None);
    };
    let revision = load_reference(
        transaction,
        PreviewPolicyRevisionWriteReference {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            project_id: ProjectId::from_uuid(row.project_id),
            source_environment_id: EnvironmentId::from_uuid(row.source_environment_id),
            source_subscription_id: SourceSubscriptionId::from_uuid(row.source_subscription_id),
            preview_policy_revision_id: PullRequestPreviewPolicyRevisionId::from_uuid(
                row.policy_revision_id,
            ),
        },
    )
    .await?;
    map_preview(row, revision.preview_authority().map_err(invariant)?).map(Some)
}

async fn store_preview(
    transaction: &PostgresTransaction,
    preview: &PullRequestPreview,
    expected: Option<PullRequestPreviewVersion>,
) -> Result<(), PostgresPersistenceError> {
    let policy = &preview.policy_authority.policy;
    let (head_provider, head_url, head_identity) =
        preview
            .head_repository
            .as_ref()
            .map_or((None, None, None), |repository| {
                (
                    Some(repository.provider().as_str()),
                    Some(repository.canonical_url()),
                    Some(repository.identity()),
                )
            });
    let (status, cleanup_reason, cleanup_requested_at) = status_values(&preview.status);
    let affected = if let Some(expected) = expected {
        execute(
            transaction,
            sql_query::<()>(
                "update developer_pull_request_previews set head_repository_provider = ",
            )
            .bind(head_provider)
            .append(", head_repository_url = ")
            .bind(head_url)
            .append(", head_repository_identity = ")
            .bind(head_identity)
            .append(", head_branch = ")
            .bind(preview.head_branch.as_str())
            .append(", head_commit_sha = ")
            .bind(preview.head_commit_sha.as_str())
            .append(", last_provider_updated_at = ")
            .bind(preview.last_provider_updated_at)
            .append(", last_change_kind = ")
            .bind(preview.last_change_kind.as_str())
            .append(", last_merged = ")
            .bind(preview.last_merged)
            .append(", expires_at = ")
            .bind(preview.expires_at)
            .append(", status = ")
            .bind(status)
            .append(", cleanup_reason = ")
            .bind(cleanup_reason)
            .append(", cleanup_requested_at = ")
            .bind(cleanup_requested_at)
            .append(", aggregate_version = ")
            .bind(preview.aggregate_version)
            .append(" where organization_id = ")
            .bind(policy.organization_id.as_uuid())
            .append(" and id = ")
            .bind(preview.id.as_uuid())
            .append(" and aggregate_version = ")
            .bind(expected.aggregate_version),
        )
        .await?
    } else {
        execute(
            transaction,
            sql_query::<()>("insert into developer_pull_request_previews (organization_id, project_id, source_environment_id, source_subscription_id, id, environment_id, policy_revision_id, pull_request_id, pull_request_number, head_repository_provider, head_repository_url, head_repository_identity, head_branch, head_commit_sha, provider_created_at, last_provider_updated_at, last_change_kind, last_merged, expires_at, status, cleanup_reason, cleanup_requested_at, aggregate_version) values (")
                .bind(policy.organization_id.as_uuid())
                .append(", ")
                .bind(policy.project_id.as_uuid())
                .append(", ")
                .bind(preview.policy_authority.source_environment_id.as_uuid())
                .append(", ")
                .bind(policy.source_subscription_id.as_uuid())
                .append(", ")
                .bind(preview.id.as_uuid())
                .append(", ")
                .bind(preview.environment_id.as_uuid())
                .append(", ")
                .bind(preview.policy_authority.revision_id.as_uuid())
                .append(", ")
                .bind(preview.pull_request_id)
                .append(", ")
                .bind(preview.pull_request_number)
                .append(", ")
                .bind(head_provider)
                .append(", ")
                .bind(head_url)
                .append(", ")
                .bind(head_identity)
                .append(", ")
                .bind(preview.head_branch.as_str())
                .append(", ")
                .bind(preview.head_commit_sha.as_str())
                .append(", ")
                .bind(preview.provider_created_at)
                .append(", ")
                .bind(preview.last_provider_updated_at)
                .append(", ")
                .bind(preview.last_change_kind.as_str())
                .append(", ")
                .bind(preview.last_merged)
                .append(", ")
                .bind(preview.expires_at)
                .append(", ")
                .bind(status)
                .append(", ")
                .bind(cleanup_reason)
                .append(", ")
                .bind(cleanup_requested_at)
                .append(", ")
                .bind(preview.aggregate_version)
                .append(") on conflict do nothing"),
        )
        .await?
    };
    if affected != 1 {
        return Err(conflict(
            "pull-request Preview advanced before its CAS mutation",
        ));
    }
    Ok(())
}

async fn insert_receipt(
    transaction: &PostgresTransaction,
    receipt: &PullRequestPreviewProjectionReceipt,
) -> Result<u64, PostgresPersistenceError> {
    execute(
        transaction,
        sql_query::<()>("insert into developer_pull_request_change_projections (organization_id, source_pull_request_change_id, project_id, source_environment_id, source_subscription_id, pull_request_id, pull_request_number, fact_digest, fact_occurred_at, policy_revision_id, preview_id, preview_aggregate_version, outcome) values (")
            .bind(receipt.organization_id.as_uuid())
            .append(", ")
            .bind(receipt.source_pull_request_change_id.as_uuid())
            .append(", ")
            .bind(receipt.project_id.as_uuid())
            .append(", ")
            .bind(receipt.source_environment_id.as_uuid())
            .append(", ")
            .bind(receipt.source_subscription_id.as_uuid())
            .append(", ")
            .bind(receipt.pull_request_id)
            .append(", ")
            .bind(receipt.pull_request_number)
            .append(", ")
            .bind(receipt.fact_digest.as_str())
            .append(", ")
            .bind(receipt.fact_occurred_at)
            .append(", ")
            .bind(receipt.policy_revision_id.map(|id| id.as_uuid()))
            .append(", ")
            .bind(receipt.preview_id.map(|id| id.as_uuid()))
            .append(", ")
            .bind(receipt.preview_aggregate_version)
            .append(", ")
            .bind(receipt.outcome.as_str())
            .append(") on conflict do nothing"),
    )
    .await
}

fn exact_replay(
    existing: PullRequestPreviewProjectionReceipt,
    candidate: &PullRequestPreviewProjectionReceipt,
) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, PostgresPersistenceError> {
    if !existing.matches_fact(&candidate.fingerprint()) {
        return Err(conflict(
            "Sources pull-request fact ID changed content or owner binding",
        ));
    }
    Ok(IdempotentWrite {
        value: existing,
        replayed: true,
    })
}

fn status_values(
    status: &PullRequestPreviewStatus,
) -> (&'static str, Option<&'static str>, Option<DateTime<Utc>>) {
    match status {
        PullRequestPreviewStatus::Active => ("active", None, None),
        PullRequestPreviewStatus::CleanupRequired {
            reason,
            requested_at,
        } => (
            "cleanup_required",
            Some(reason.as_str()),
            Some(*requested_at),
        ),
    }
}

struct PullRequestPreviewRow {
    organization_id: Uuid,
    project_id: Uuid,
    source_environment_id: Uuid,
    source_subscription_id: Uuid,
    id: Uuid,
    environment_id: Uuid,
    policy_revision_id: Uuid,
    pull_request_id: u64,
    pull_request_number: u64,
    head_repository_provider: Option<String>,
    head_repository_url: Option<String>,
    head_repository_identity: Option<String>,
    head_branch: String,
    head_commit_sha: String,
    provider_created_at: DateTime<Utc>,
    last_provider_updated_at: DateTime<Utc>,
    last_change_kind: String,
    last_merged: bool,
    expires_at: DateTime<Utc>,
    status: String,
    cleanup_reason: Option<String>,
    cleanup_requested_at: Option<DateTime<Utc>>,
    aggregate_version: u64,
}

impl FromRow for PullRequestPreviewRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            source_environment_id: decode(row, 2)?,
            source_subscription_id: decode(row, 3)?,
            id: decode(row, 4)?,
            environment_id: decode(row, 5)?,
            policy_revision_id: decode(row, 6)?,
            pull_request_id: decode(row, 7)?,
            pull_request_number: decode(row, 8)?,
            head_repository_provider: decode(row, 9)?,
            head_repository_url: decode(row, 10)?,
            head_repository_identity: decode(row, 11)?,
            head_branch: decode(row, 12)?,
            head_commit_sha: decode(row, 13)?,
            provider_created_at: decode(row, 14)?,
            last_provider_updated_at: decode(row, 15)?,
            last_change_kind: decode(row, 16)?,
            last_merged: decode(row, 17)?,
            expires_at: decode(row, 18)?,
            status: decode(row, 19)?,
            cleanup_reason: decode(row, 20)?,
            cleanup_requested_at: decode(row, 21)?,
            aggregate_version: decode(row, 22)?,
        })
    }
}

struct PullRequestPreviewProjectionReceiptRow {
    organization_id: Uuid,
    source_pull_request_change_id: Uuid,
    project_id: Uuid,
    source_environment_id: Uuid,
    source_subscription_id: Uuid,
    pull_request_id: u64,
    pull_request_number: u64,
    fact_digest: String,
    fact_occurred_at: DateTime<Utc>,
    policy_revision_id: Option<Uuid>,
    preview_id: Option<Uuid>,
    preview_aggregate_version: Option<u64>,
    outcome: String,
}

impl FromRow for PullRequestPreviewProjectionReceiptRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            source_pull_request_change_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            source_environment_id: decode(row, 3)?,
            source_subscription_id: decode(row, 4)?,
            pull_request_id: decode(row, 5)?,
            pull_request_number: decode(row, 6)?,
            fact_digest: decode(row, 7)?,
            fact_occurred_at: decode(row, 8)?,
            policy_revision_id: decode(row, 9)?,
            preview_id: decode(row, 10)?,
            preview_aggregate_version: decode(row, 11)?,
            outcome: decode(row, 12)?,
        })
    }
}

fn map_preview(
    row: PullRequestPreviewRow,
    policy_authority: crate::modules::developer_workflows::domain::PullRequestPreviewPolicyAuthority,
) -> Result<PullRequestPreview, PostgresPersistenceError> {
    let head_repository = match (
        row.head_repository_provider,
        row.head_repository_url,
        row.head_repository_identity,
    ) {
        (None, None, None) => None,
        (Some(provider), Some(url), Some(identity)) => {
            let repository =
                GitRepository::parse(GitProvider::parse(&provider).map_err(invariant)?, &url)
                    .map_err(invariant)?;
            if repository.identity() != identity {
                return Err(invariant(
                    "stored Preview head repository identity drifted from its URL".into(),
                ));
            }
            Some(repository)
        }
        _ => {
            return Err(invariant(
                "stored Preview head repository columns are incomplete".into(),
            ))
        }
    };
    let status = match (
        row.status.as_str(),
        row.cleanup_reason.as_deref(),
        row.cleanup_requested_at,
    ) {
        ("active", None, None) => PullRequestPreviewStatus::Active,
        ("cleanup_required", Some(reason), Some(requested_at)) => {
            PullRequestPreviewStatus::CleanupRequired {
                reason: PreviewCleanupReason::parse(reason).map_err(invariant)?,
                requested_at,
            }
        }
        _ => {
            return Err(invariant(
                "stored Preview status evidence is invalid".into(),
            ))
        }
    };
    PullRequestPreview::restore(PullRequestPreview {
        policy_authority,
        id: PullRequestPreviewId::from_uuid(row.id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        pull_request_id: row.pull_request_id,
        pull_request_number: row.pull_request_number,
        head_repository,
        head_branch: crate::modules::developer_workflows::domain::GitBranch::parse(row.head_branch)
            .map_err(invariant)?,
        head_commit_sha: GitCommitSha::parse(row.head_commit_sha).map_err(invariant)?,
        provider_created_at: row.provider_created_at,
        last_provider_updated_at: row.last_provider_updated_at,
        last_change_kind: PullRequestChangeKind::parse(&row.last_change_kind).map_err(invariant)?,
        last_merged: row.last_merged,
        expires_at: row.expires_at,
        status,
        aggregate_version: row.aggregate_version,
    })
    .map_err(invariant)
}

fn map_receipt(
    row: PullRequestPreviewProjectionReceiptRow,
) -> Result<PullRequestPreviewProjectionReceipt, PostgresPersistenceError> {
    PullRequestPreviewProjectionReceipt::restore(PullRequestPreviewProjectionReceipt {
        source_pull_request_change_id: SourcePullRequestChangeId::from_uuid(
            row.source_pull_request_change_id,
        ),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        source_environment_id: EnvironmentId::from_uuid(row.source_environment_id),
        source_subscription_id: SourceSubscriptionId::from_uuid(row.source_subscription_id),
        pull_request_id: row.pull_request_id,
        pull_request_number: row.pull_request_number,
        fact_digest: Sha256Digest::parse(row.fact_digest).map_err(invariant)?,
        fact_occurred_at: row.fact_occurred_at,
        policy_revision_id: row
            .policy_revision_id
            .map(PullRequestPreviewPolicyRevisionId::from_uuid),
        preview_id: row.preview_id.map(PullRequestPreviewId::from_uuid),
        preview_aggregate_version: row.preview_aggregate_version,
        outcome: PullRequestPreviewProjectionOutcome::parse(&row.outcome).map_err(invariant)?,
    })
    .map_err(invariant)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn invariant(error: String) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(format!(
        "stored pull-request Preview projection is invalid: {error}"
    ))
}

fn conflict(message: &str) -> PostgresPersistenceError {
    PostgresPersistenceError::Repository(RepositoryError::Conflict(message.into()))
}
