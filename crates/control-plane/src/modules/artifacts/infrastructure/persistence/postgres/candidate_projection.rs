use super::{
    decode, map_row, persist_build, BuildRunRow, PostgresBuildRunRepository, SELECT_BUILDS,
};
use crate::infrastructure::{execute, fetch_optional, transaction_error, PostgresPersistenceError};
use crate::modules::artifacts::application::{
    BuildCandidate, BuildCandidateEvidence, IBuildCandidateProjectionPort,
    IPreviewBuildLifecycleProjectionPort, PreviewBuildLifecycleProjectionOutcome,
    PreviewBuildLifecycleProjectionReceipt, PreviewBuildLifecycleState, PreviewBuildRetirement,
    PreviewBuildSourceRevision, ProjectPreviewBuildLifecycle,
};
use crate::modules::artifacts::domain::repositories::validate_build_run_transition;
use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus, BuildSubject};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, EnvironmentId, GitCommitSha, IdempotentWrite,
    OrganizationId, ProjectId, PullRequestPreviewId, RepositoryError, Sha256Digest,
    SourcePullRequestChangeId, SourceRevisionId, SourceSubscriptionId,
};
use a3s_orm::{sql_query, DecodeError, FromRow, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) struct BuildCandidateRow {
    organization_id: Uuid,
    subject_kind: String,
    subject_id: Uuid,
    preview_id: Option<Uuid>,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    source_revision_id: Option<Uuid>,
    asset_id: Option<Uuid>,
    asset_release_id: Option<Uuid>,
    repository_identity: Option<String>,
    commit_sha: String,
    owner_input_digest: String,
    requested_at: DateTime<Utc>,
}

impl FromRow for BuildCandidateRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            subject_kind: decode(row, 1)?,
            subject_id: decode(row, 2)?,
            preview_id: decode(row, 3)?,
            project_id: decode(row, 4)?,
            environment_id: decode(row, 5)?,
            source_revision_id: decode(row, 6)?,
            asset_id: decode(row, 7)?,
            asset_release_id: decode(row, 8)?,
            repository_identity: decode(row, 9)?,
            commit_sha: decode(row, 10)?,
            owner_input_digest: decode(row, 11)?,
            requested_at: decode(row, 12)?,
        })
    }
}

struct PreviewBuildLifecycleProjectionReceiptRow {
    organization_id: Uuid,
    preview_id: Uuid,
    preview_aggregate_version: u64,
    lifecycle_event_id: Uuid,
    correlation_id: Uuid,
    lifecycle_causation_id: Uuid,
    source_pull_request_change_id: Uuid,
    project_id: Uuid,
    source_environment_id: Uuid,
    source_subscription_id: Uuid,
    preview_environment_id: Uuid,
    state: String,
    source_revision_id: Option<Uuid>,
    repository_identity: Option<String>,
    commit_sha: Option<String>,
    recipe_digest: Option<String>,
    source_revision_accepted_at: Option<DateTime<Utc>>,
    fact_occurred_at: DateTime<Utc>,
    outcome: String,
    retirement: String,
    retired_source_revision_id: Option<Uuid>,
    retired_build_run_id: Option<Uuid>,
}

impl FromRow for PreviewBuildLifecycleProjectionReceiptRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            preview_id: decode(row, 1)?,
            preview_aggregate_version: decode(row, 2)?,
            lifecycle_event_id: decode(row, 3)?,
            correlation_id: decode(row, 4)?,
            lifecycle_causation_id: decode(row, 5)?,
            source_pull_request_change_id: decode(row, 6)?,
            project_id: decode(row, 7)?,
            source_environment_id: decode(row, 8)?,
            source_subscription_id: decode(row, 9)?,
            preview_environment_id: decode(row, 10)?,
            state: decode(row, 11)?,
            source_revision_id: decode(row, 12)?,
            repository_identity: decode(row, 13)?,
            commit_sha: decode(row, 14)?,
            recipe_digest: decode(row, 15)?,
            source_revision_accepted_at: decode(row, 16)?,
            fact_occurred_at: decode(row, 17)?,
            outcome: decode(row, 18)?,
            retirement: decode(row, 19)?,
            retired_source_revision_id: decode(row, 20)?,
            retired_build_run_id: decode(row, 21)?,
        })
    }
}

pub(super) const SELECT_BUILD_CANDIDATES: &str = "select c.organization_id, c.subject_kind, c.subject_id, c.preview_id, c.project_id, c.environment_id, c.source_revision_id, c.asset_id, c.asset_release_id, c.repository_identity, c.commit_sha, c.owner_input_digest, c.requested_at from artifact_build_candidates c";
const SELECT_PREVIEW_BUILD_LIFECYCLE_RECEIPTS: &str = "select organization_id, preview_id, preview_aggregate_version, lifecycle_event_id, correlation_id, lifecycle_causation_id, source_pull_request_change_id, project_id, source_environment_id, source_subscription_id, preview_environment_id, state, source_revision_id, repository_identity, commit_sha, recipe_digest, source_revision_accepted_at, fact_occurred_at, outcome, retirement, retired_source_revision_id, retired_build_run_id from artifact_preview_build_lifecycle_projections";

#[async_trait]
impl IBuildCandidateProjectionPort for PostgresBuildRunRepository {
    async fn project_candidate(&self, candidate: BuildCandidate) -> Result<(), RepositoryError> {
        candidate.validate().map_err(|error| {
            RepositoryError::Storage(format!("invalid build candidate: {error}"))
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { insert_candidate_projection(transaction, &candidate).await })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IPreviewBuildLifecycleProjectionPort for PostgresBuildRunRepository {
    async fn project_preview_build_lifecycle(
        &self,
        input: ProjectPreviewBuildLifecycle,
    ) -> Result<IdempotentWrite<PreviewBuildLifecycleProjectionReceipt>, RepositoryError> {
        input.validate().map_err(|error| {
            RepositoryError::Storage(format!(
                "invalid Preview build lifecycle projection: {error}"
            ))
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    transaction
                        .advisory_xact_lock(
                            "a3s.cloud.artifact-preview-build-lifecycle",
                            &format!("{}:{}", input.organization_id, input.preview_id),
                        )
                        .await?;
                    if let Some(existing) = load_preview_build_lifecycle_receipt(
                        transaction,
                        input.organization_id,
                        input.preview_id,
                        input.preview_aggregate_version,
                    )
                    .await?
                    {
                        return exact_preview_build_lifecycle_replay(existing, &input);
                    }
                    if load_preview_build_lifecycle_receipt_by_event(
                        transaction,
                        input.lifecycle_event_id,
                    )
                    .await?
                    .is_some()
                    {
                        return Err(preview_build_projection_conflict(
                            "Preview lifecycle event identity was reused for another Artifacts projection",
                        ));
                    }

                    let latest = load_latest_preview_build_lifecycle_receipt(
                        transaction,
                        input.organization_id,
                        input.preview_id,
                    )
                    .await?;
                    let decision = input.decide(latest.as_ref()).map_err(|error| {
                        preview_build_projection_conflict(&format!(
                            "Preview build projection authority is invalid: {error}"
                        ))
                    })?;
                    let retirement = if decision.outcome
                        == PreviewBuildLifecycleProjectionOutcome::IgnoredStale
                    {
                        PreviewBuildRetirement::NotRequired
                    } else if let Some(source_revision_id) = decision.retired_source_revision_id {
                        let candidate = load_source_candidate_for_update(
                            transaction,
                            input.organization_id,
                            source_revision_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            preview_build_projection_invariant(
                                "retired Preview SourceRevision has no Artifacts candidate".into(),
                            )
                        })?;
                        if candidate.preview_id() != Some(input.preview_id) {
                            return Err(preview_build_projection_conflict(
                                "retired Preview SourceRevision belongs to another Preview",
                            ));
                        }
                        match load_latest_build_for_candidate_for_update(
                            transaction,
                            &candidate,
                        )
                        .await?
                        {
                            None => PreviewBuildRetirement::PendingSuppressed {
                                source_revision_id,
                            },
                            Some(build) if build.status.is_terminal() => {
                                PreviewBuildRetirement::TerminalObserved {
                                    source_revision_id,
                                    build_run_id: build.id,
                                }
                            }
                            Some(build) if build.cancellation_requested_at.is_some() => {
                                PreviewBuildRetirement::CancellationRequested {
                                    source_revision_id,
                                    build_run_id: build.id,
                                }
                            }
                            Some(build) => {
                                let expected_version = build.aggregate_version;
                                let mut cancellation = build.clone();
                                cancellation
                                    .request_cancellation(std::cmp::max(
                                        input.fact_occurred_at,
                                        build.updated_at,
                                    ))
                                    .map_err(|error| {
                                        preview_build_projection_conflict(&error)
                                    })?;
                                validate_build_run_transition(
                                    &build,
                                    &cancellation,
                                    expected_version,
                                )
                                .map_err(PostgresPersistenceError::Repository)?;
                                persist_build(transaction, &cancellation, expected_version).await?;
                                PreviewBuildRetirement::CancellationRequested {
                                    source_revision_id,
                                    build_run_id: build.id,
                                }
                            }
                        }
                    } else {
                        PreviewBuildRetirement::NotRequired
                    };
                    if let Some(candidate) = decision.candidate {
                        insert_candidate_projection(transaction, &candidate).await?;
                    }
                    let receipt = PreviewBuildLifecycleProjectionReceipt::from_input(
                        &input,
                        decision.outcome,
                        retirement,
                    )
                    .map_err(preview_build_projection_invariant)?;
                    insert_preview_build_lifecycle_receipt(transaction, &receipt).await?;
                    Ok(IdempotentWrite {
                        value: receipt,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn load_preview_build_lifecycle_receipt(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    preview_id: PullRequestPreviewId,
    preview_aggregate_version: u64,
) -> Result<Option<PreviewBuildLifecycleProjectionReceipt>, PostgresPersistenceError> {
    fetch_optional::<PreviewBuildLifecycleProjectionReceiptRow, _>(
        transaction,
        sql_query::<PreviewBuildLifecycleProjectionReceiptRow>(
            SELECT_PREVIEW_BUILD_LIFECYCLE_RECEIPTS,
        )
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and preview_id = ")
        .bind(preview_id.as_uuid())
        .append(" and preview_aggregate_version = ")
        .bind(preview_aggregate_version),
    )
    .await?
    .map(map_preview_build_lifecycle_receipt)
    .transpose()
}

async fn load_preview_build_lifecycle_receipt_by_event(
    transaction: &a3s_orm::PostgresTransaction,
    lifecycle_event_id: Uuid,
) -> Result<Option<PreviewBuildLifecycleProjectionReceipt>, PostgresPersistenceError> {
    fetch_optional::<PreviewBuildLifecycleProjectionReceiptRow, _>(
        transaction,
        sql_query::<PreviewBuildLifecycleProjectionReceiptRow>(
            SELECT_PREVIEW_BUILD_LIFECYCLE_RECEIPTS,
        )
        .append(" where lifecycle_event_id = ")
        .bind(lifecycle_event_id),
    )
    .await?
    .map(map_preview_build_lifecycle_receipt)
    .transpose()
}

async fn load_latest_preview_build_lifecycle_receipt(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    preview_id: PullRequestPreviewId,
) -> Result<Option<PreviewBuildLifecycleProjectionReceipt>, PostgresPersistenceError> {
    fetch_optional::<PreviewBuildLifecycleProjectionReceiptRow, _>(
        transaction,
        sql_query::<PreviewBuildLifecycleProjectionReceiptRow>(
            SELECT_PREVIEW_BUILD_LIFECYCLE_RECEIPTS,
        )
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and preview_id = ")
        .bind(preview_id.as_uuid())
        .append(" order by preview_aggregate_version desc limit 1"),
    )
    .await?
    .map(map_preview_build_lifecycle_receipt)
    .transpose()
}

async fn load_source_candidate_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    source_revision_id: SourceRevisionId,
) -> Result<Option<BuildCandidate>, PostgresPersistenceError> {
    fetch_optional::<BuildCandidateRow, _>(
        transaction,
        sql_query::<BuildCandidateRow>(SELECT_BUILD_CANDIDATES)
            .append(" where c.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and c.subject_kind = 'external_source_revision' and c.subject_id = ")
            .bind(source_revision_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(map_candidate_row)
    .transpose()
}

pub(super) async fn load_latest_build_for_candidate_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    candidate: &BuildCandidate,
) -> Result<Option<BuildRun>, PostgresPersistenceError> {
    let values = candidate_values(candidate);
    let query = sql_query::<BuildRunRow>(SELECT_BUILDS)
        .append(" where b.organization_id = ")
        .bind(values.organization_id)
        .append(" and b.subject_kind = ")
        .bind(values.subject_kind);
    let query = match candidate.subject() {
        BuildSubject::ExternalSourceRevision {
            source_revision_id, ..
        } => query
            .append(" and b.source_revision_id = ")
            .bind(source_revision_id.as_uuid()),
        BuildSubject::AssetRelease {
            asset_release_id, ..
        } => query
            .append(" and b.asset_release_id = ")
            .bind(asset_release_id.as_uuid()),
    };
    fetch_optional::<BuildRunRow, _>(
        transaction,
        query.append(" order by b.attempt desc limit 1 for update"),
    )
    .await?
    .map(map_row)
    .transpose()
    .map_err(PostgresPersistenceError::Repository)
}

pub(super) async fn preview_candidate_is_current_postgres(
    transaction: &a3s_orm::PostgresTransaction,
    candidate: &BuildCandidate,
) -> Result<bool, PostgresPersistenceError> {
    let Some(preview_id) = candidate.preview_id() else {
        return Ok(true);
    };
    let Some(head) = load_latest_preview_build_lifecycle_receipt(
        transaction,
        candidate.organization_id(),
        preview_id,
    )
    .await?
    else {
        return Ok(false);
    };
    let BuildSubject::ExternalSourceRevision {
        project_id,
        environment_id,
        source_revision_id,
    } = candidate.subject()
    else {
        return Ok(false);
    };
    Ok(
        head.outcome == PreviewBuildLifecycleProjectionOutcome::Applied
            && head.state == PreviewBuildLifecycleState::Active
            && head.source_revision_id() == Some(source_revision_id)
            && head.project_id == project_id
            && head.preview_environment_id == environment_id,
    )
}

pub(super) async fn preview_retry_requested_at_postgres(
    transaction: &a3s_orm::PostgresTransaction,
    candidate: &BuildCandidate,
    previous: &BuildRun,
) -> Result<Option<DateTime<Utc>>, PostgresPersistenceError> {
    let Some(preview_id) = candidate.preview_id() else {
        return Ok(None);
    };
    if !matches!(
        previous.status,
        BuildRunStatus::Failed | BuildRunStatus::Cancelled
    ) {
        return Ok(None);
    }
    let Some(head) = load_latest_preview_build_lifecycle_receipt(
        transaction,
        candidate.organization_id(),
        preview_id,
    )
    .await?
    else {
        return Ok(None);
    };
    if head.outcome != PreviewBuildLifecycleProjectionOutcome::Applied
        || head.state != PreviewBuildLifecycleState::Active
        || head.source_revision_id() != candidate.subject().source_revision_id()
    {
        return Ok(None);
    }
    let Some(source_revision_id) = candidate.subject().source_revision_id() else {
        return Ok(None);
    };
    let authority = fetch_optional::<PreviewBuildLifecycleProjectionReceiptRow, _>(
        transaction,
        sql_query::<PreviewBuildLifecycleProjectionReceiptRow>(
            SELECT_PREVIEW_BUILD_LIFECYCLE_RECEIPTS,
        )
        .append(" where organization_id = ")
        .bind(candidate.organization_id().as_uuid())
        .append(" and preview_id = ")
        .bind(preview_id.as_uuid())
        .append(" and preview_aggregate_version < ")
        .bind(head.preview_aggregate_version)
        .append(" and retired_source_revision_id = ")
        .bind(source_revision_id.as_uuid())
        .append(" and retired_build_run_id = ")
        .bind(previous.id.as_uuid())
        .append(" and retirement in ('cancellation_requested', 'terminal_observed') order by preview_aggregate_version desc limit 1"),
    )
    .await?
    .map(map_preview_build_lifecycle_receipt)
    .transpose()?;
    Ok(authority.map(|_| std::cmp::max(head.fact_occurred_at, previous.updated_at)))
}

async fn insert_preview_build_lifecycle_receipt(
    transaction: &a3s_orm::PostgresTransaction,
    receipt: &PreviewBuildLifecycleProjectionReceipt,
) -> Result<(), PostgresPersistenceError> {
    let (source_revision_id, repository_identity, commit_sha, recipe_digest, accepted_at) =
        match receipt.source_revision.as_ref() {
            Some(revision) => (
                Some(revision.source_revision_id.as_uuid()),
                Some(revision.repository_identity.as_str()),
                Some(revision.commit_sha.as_str()),
                Some(revision.recipe_digest.as_str()),
                Some(revision.accepted_at),
            ),
            None => (None, None, None, None, None),
        };
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into artifact_preview_build_lifecycle_projections (organization_id, preview_id, preview_aggregate_version, lifecycle_event_id, correlation_id, lifecycle_causation_id, source_pull_request_change_id, project_id, source_environment_id, source_subscription_id, preview_environment_id, state, source_revision_id, repository_identity, commit_sha, recipe_digest, source_revision_accepted_at, fact_occurred_at, outcome, retirement, retired_source_revision_id, retired_build_run_id) values (",
        )
        .bind(receipt.organization_id.as_uuid())
        .append(", ")
        .bind(receipt.preview_id.as_uuid())
        .append(", ")
        .bind(receipt.preview_aggregate_version)
        .append(", ")
        .bind(receipt.lifecycle_event_id)
        .append(", ")
        .bind(receipt.correlation_id)
        .append(", ")
        .bind(receipt.lifecycle_causation_id)
        .append(", ")
        .bind(receipt.source_pull_request_change_id.as_uuid())
        .append(", ")
        .bind(receipt.project_id.as_uuid())
        .append(", ")
        .bind(receipt.source_environment_id.as_uuid())
        .append(", ")
        .bind(receipt.source_subscription_id.as_uuid())
        .append(", ")
        .bind(receipt.preview_environment_id.as_uuid())
        .append(", ")
        .bind(receipt.state.as_str())
        .append(", ")
        .bind(source_revision_id)
        .append(", ")
        .bind(repository_identity)
        .append(", ")
        .bind(commit_sha)
        .append(", ")
        .bind(recipe_digest)
        .append(", ")
        .bind(accepted_at)
        .append(", ")
        .bind(receipt.fact_occurred_at)
        .append(", ")
        .bind(receipt.outcome.as_str())
        .append(", ")
        .bind(receipt.retirement.as_str())
        .append(", ")
        .bind(
            receipt
                .retirement
                .source_revision_id()
                .map(SourceRevisionId::as_uuid),
        )
        .append(", ")
        .bind(
            receipt
                .retirement
                .build_run_id()
                .map(BuildRunId::as_uuid),
        )
        .append(") on conflict (organization_id, preview_id, preview_aggregate_version) do nothing"),
    )
    .await;
    match inserted {
        Ok(1) => Ok(()),
        Ok(0) => Err(preview_build_projection_conflict(
            "Preview aggregate version was committed concurrently",
        )),
        Ok(rows) => Err(preview_build_projection_invariant(format!(
            "committing Preview build lifecycle receipt affected {rows} rows"
        ))),
        Err(error) if crate::infrastructure::is_unique_violation(&error) => {
            Err(preview_build_projection_conflict(
                "Preview lifecycle event identity was committed concurrently",
            ))
        }
        Err(error) => Err(error),
    }
}

fn map_preview_build_lifecycle_receipt(
    row: PreviewBuildLifecycleProjectionReceiptRow,
) -> Result<PreviewBuildLifecycleProjectionReceipt, PostgresPersistenceError> {
    let source_revision = match (
        row.source_revision_id,
        row.repository_identity,
        row.commit_sha,
        row.recipe_digest,
        row.source_revision_accepted_at,
    ) {
        (
            Some(source_revision_id),
            Some(repository_identity),
            Some(commit_sha),
            Some(recipe_digest),
            Some(accepted_at),
        ) => Some(PreviewBuildSourceRevision {
            source_revision_id: SourceRevisionId::from_uuid(source_revision_id),
            repository_identity,
            commit_sha: GitCommitSha::parse(commit_sha)
                .map_err(preview_build_projection_invariant)?,
            recipe_digest: Sha256Digest::parse(recipe_digest)
                .map_err(preview_build_projection_invariant)?,
            accepted_at,
        }),
        (None, None, None, None, None) => None,
        _ => {
            return Err(preview_build_projection_invariant(
                "stored Preview build SourceRevision evidence is incomplete".into(),
            ))
        }
    };
    PreviewBuildLifecycleProjectionReceipt::restore(PreviewBuildLifecycleProjectionReceipt {
        lifecycle_event_id: row.lifecycle_event_id,
        correlation_id: row.correlation_id,
        lifecycle_causation_id: row.lifecycle_causation_id,
        source_pull_request_change_id: SourcePullRequestChangeId::from_uuid(
            row.source_pull_request_change_id,
        ),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        source_environment_id: EnvironmentId::from_uuid(row.source_environment_id),
        source_subscription_id: SourceSubscriptionId::from_uuid(row.source_subscription_id),
        preview_id: PullRequestPreviewId::from_uuid(row.preview_id),
        preview_aggregate_version: row.preview_aggregate_version,
        preview_environment_id: EnvironmentId::from_uuid(row.preview_environment_id),
        state: PreviewBuildLifecycleState::parse(&row.state)
            .map_err(preview_build_projection_invariant)?,
        source_revision,
        fact_occurred_at: row.fact_occurred_at,
        outcome: PreviewBuildLifecycleProjectionOutcome::parse(&row.outcome)
            .map_err(preview_build_projection_invariant)?,
        retirement: PreviewBuildRetirement::restore(
            &row.retirement,
            row.retired_source_revision_id
                .map(SourceRevisionId::from_uuid),
            row.retired_build_run_id.map(BuildRunId::from_uuid),
        )
        .map_err(preview_build_projection_invariant)?,
    })
    .map_err(preview_build_projection_invariant)
}

fn exact_preview_build_lifecycle_replay(
    receipt: PreviewBuildLifecycleProjectionReceipt,
    input: &ProjectPreviewBuildLifecycle,
) -> Result<IdempotentWrite<PreviewBuildLifecycleProjectionReceipt>, PostgresPersistenceError> {
    if !receipt.matches_input(input) {
        return Err(preview_build_projection_conflict(
            "Preview aggregate version changed its Artifacts build projection",
        ));
    }
    Ok(IdempotentWrite {
        value: receipt,
        replayed: true,
    })
}

fn preview_build_projection_invariant(error: String) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(format!(
        "Preview build lifecycle projection is invalid: {error}"
    ))
}

fn preview_build_projection_conflict(message: &str) -> PostgresPersistenceError {
    PostgresPersistenceError::Repository(RepositoryError::Conflict(message.into()))
}

async fn insert_candidate_projection(
    transaction: &a3s_orm::PostgresTransaction,
    candidate: &BuildCandidate,
) -> Result<(), PostgresPersistenceError> {
    let values = candidate_values(candidate);
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into artifact_build_candidates (organization_id, subject_kind, subject_id, preview_id, project_id, environment_id, source_revision_id, asset_id, asset_release_id, repository_identity, commit_sha, owner_input_digest, requested_at) values (",
        )
        .bind(values.organization_id)
        .append(", ")
        .bind(values.subject_kind)
        .append(", ")
        .bind(values.subject_id)
        .append(", ")
        .bind(values.preview_id)
        .append(", ")
        .bind(values.project_id)
        .append(", ")
        .bind(values.environment_id)
        .append(", ")
        .bind(values.source_revision_id)
        .append(", ")
        .bind(values.asset_id)
        .append(", ")
        .bind(values.asset_release_id)
        .append(", ")
        .bind(values.repository_identity)
        .append(", ")
        .bind(values.commit_sha)
        .append(", ")
        .bind(values.owner_input_digest)
        .append(", ")
        .bind(values.requested_at)
        .append(") on conflict (organization_id, subject_kind, subject_id) do nothing"),
    )
    .await?;
    if inserted == 1 {
        return Ok(());
    }
    if inserted != 0 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "projecting a build candidate affected {inserted} rows"
        )));
    }
    let existing = fetch_optional::<BuildCandidateRow, _>(
        transaction,
        sql_query::<BuildCandidateRow>(SELECT_BUILD_CANDIDATES)
            .append(" where c.organization_id = ")
            .bind(values.organization_id)
            .append(" and c.subject_kind = ")
            .bind(values.subject_kind)
            .append(" and c.subject_id = ")
            .bind(values.subject_id)
            .append(" for update"),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "conflicting build candidate could not be reloaded".into(),
        )
    })?;
    if map_candidate_row(existing)? == *candidate {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "build candidate fact conflicts with its existing projection".into(),
            ),
        ))
    }
}

#[derive(Clone, Copy)]
struct BuildCandidateValues<'a> {
    organization_id: Uuid,
    subject_kind: &'static str,
    subject_id: Uuid,
    preview_id: Option<Uuid>,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    source_revision_id: Option<Uuid>,
    asset_id: Option<Uuid>,
    asset_release_id: Option<Uuid>,
    repository_identity: Option<&'a str>,
    commit_sha: &'a str,
    owner_input_digest: &'a str,
    requested_at: DateTime<Utc>,
}

fn candidate_values(candidate: &BuildCandidate) -> BuildCandidateValues<'_> {
    match (candidate.subject(), candidate.evidence()) {
        (
            BuildSubject::ExternalSourceRevision {
                project_id,
                environment_id,
                source_revision_id,
            },
            BuildCandidateEvidence::ExternalSourceRevision {
                repository_identity,
                commit_sha,
                recipe_digest,
            },
        ) => BuildCandidateValues {
            organization_id: candidate.organization_id().as_uuid(),
            subject_kind: "external_source_revision",
            subject_id: source_revision_id.as_uuid(),
            preview_id: candidate.preview_id().map(PullRequestPreviewId::as_uuid),
            project_id: Some(project_id.as_uuid()),
            environment_id: Some(environment_id.as_uuid()),
            source_revision_id: Some(source_revision_id.as_uuid()),
            asset_id: None,
            asset_release_id: None,
            repository_identity: Some(repository_identity),
            commit_sha: commit_sha.as_str(),
            owner_input_digest: recipe_digest.as_str(),
            requested_at: candidate.requested_at(),
        },
        (
            BuildSubject::AssetRelease {
                asset_id,
                asset_release_id,
            },
            BuildCandidateEvidence::HostedAssetRelease {
                commit_sha,
                manifest_digest,
            },
        ) => BuildCandidateValues {
            organization_id: candidate.organization_id().as_uuid(),
            subject_kind: "asset_release",
            subject_id: asset_release_id.as_uuid(),
            preview_id: None,
            project_id: None,
            environment_id: None,
            source_revision_id: None,
            asset_id: Some(asset_id.as_uuid()),
            asset_release_id: Some(asset_release_id.as_uuid()),
            repository_identity: None,
            commit_sha: commit_sha.as_str(),
            owner_input_digest: manifest_digest.as_str(),
            requested_at: candidate.requested_at(),
        },
        _ => unreachable!("validated build candidate pairs its subject and evidence"),
    }
}

pub(super) fn map_candidate_row(
    row: BuildCandidateRow,
) -> Result<BuildCandidate, PostgresPersistenceError> {
    let BuildCandidateRow {
        organization_id,
        subject_kind,
        subject_id,
        preview_id,
        project_id,
        environment_id,
        source_revision_id,
        asset_id,
        asset_release_id,
        repository_identity,
        commit_sha,
        owner_input_digest,
        requested_at,
    } = row;
    let (subject, evidence) = match (
        subject_kind.as_str(),
        project_id,
        environment_id,
        source_revision_id,
        asset_id,
        asset_release_id,
        repository_identity,
    ) {
        (
            "external_source_revision",
            Some(project_id),
            Some(environment_id),
            Some(revision_id),
            None,
            None,
            Some(repository_identity),
        ) if subject_id == revision_id => (
            BuildSubject::external_source_revision(
                ProjectId::from_uuid(project_id),
                EnvironmentId::from_uuid(environment_id),
                SourceRevisionId::from_uuid(revision_id),
            ),
            BuildCandidateEvidence::external_source_revision(
                repository_identity,
                GitCommitSha::parse(commit_sha).map_err(PostgresPersistenceError::Invariant)?,
                Sha256Digest::parse(owner_input_digest)
                    .map_err(PostgresPersistenceError::Invariant)?,
            )
            .map_err(PostgresPersistenceError::Invariant)?,
        ),
        ("asset_release", None, None, None, Some(asset_id), Some(asset_release_id), None)
            if subject_id == asset_release_id =>
        {
            (
                BuildSubject::asset_release(
                    AssetId::from_uuid(asset_id),
                    AssetReleaseId::from_uuid(asset_release_id),
                ),
                BuildCandidateEvidence::hosted_asset_release(
                    GitCommitSha::parse(commit_sha).map_err(PostgresPersistenceError::Invariant)?,
                    Sha256Digest::parse(owner_input_digest)
                        .map_err(PostgresPersistenceError::Invariant)?,
                ),
            )
        }
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "stored build candidate has an invalid subject shape".into(),
            ))
        }
    };
    let organization_id = OrganizationId::from_uuid(organization_id);
    let candidate = match preview_id {
        Some(preview_id) => BuildCandidate::for_preview_source_revision(
            organization_id,
            subject,
            PullRequestPreviewId::from_uuid(preview_id),
            evidence,
            requested_at,
        ),
        None => BuildCandidate::new(organization_id, subject, evidence, requested_at),
    };
    candidate.map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored build candidate failed validation: {error}"
        ))
    })
}
