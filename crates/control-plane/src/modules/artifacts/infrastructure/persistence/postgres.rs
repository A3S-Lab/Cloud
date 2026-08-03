use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, store_idempotency, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::artifacts::domain::repositories::{
    validate_build_run_finalization, validate_build_run_retry, validate_build_run_transition,
    BuildRunFinalizationMode,
};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildEvidence, BuildRun, BuildRunFinalization, BuildRunStatus, BuildSubject,
    IBuildRunRepository, OciPublicationTarget, PublishedOciArtifact,
    RequestBuildCancellationBundle, RequestBuildRetryBundle, ValidatedOciBuildOutput,
};
use crate::modules::assets::infrastructure::{
    apply_hosted_release, plan_hosted_release, verify_hosted_release_unpublished, HostedReleasePlan,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use a3s_cloud_contracts::NodeBoxBuildOutput;
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_BUILDS: &str = "select b.organization_id, b.subject_kind, b.project_id, b.environment_id, b.source_revision_id, b.asset_id, b.asset_release_id, b.id, b.attempt, b.retry_of_build_run_id, b.operation_id, b.status, b.source_content_digest, b.input_artifact, b.node_id, b.command_id, b.cleanup_command_id, b.build_request_digest, b.box_build_output, b.output, b.publication_target, b.published_artifact, b.evidence_required, b.evidence, b.failure, b.aggregate_version, b.requested_at, b.updated_at, b.started_at, b.cancellation_requested_at, b.finished_at from build_runs b";

type PendingRevisionRow = (Uuid, Uuid, Uuid, Uuid, DateTime<Utc>);
type PendingAssetReleaseRow = (Uuid, Uuid, Uuid, DateTime<Utc>);

enum PendingBuild {
    External(PendingRevisionRow),
    AssetRelease(PendingAssetReleaseRow),
}

impl PendingBuild {
    fn sort_key(&self) -> (DateTime<Utc>, u8, Uuid) {
        match self {
            Self::External((_, _, _, source_revision_id, accepted_at)) => {
                (*accepted_at, 0, *source_revision_id)
            }
            Self::AssetRelease((_, _, asset_release_id, drafted_at)) => {
                (*drafted_at, 1, *asset_release_id)
            }
        }
    }
}

#[derive(Clone)]
pub struct PostgresBuildRunRepository {
    executor: PostgresExecutor,
}

impl PostgresBuildRunRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IBuildRunRepository for PostgresBuildRunRepository {
    async fn reserve_pending(
        &self,
        limit: usize,
        reserved_at: DateTime<Utc>,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let revisions = fetch_all::<PendingRevisionRow, _>(
                        transaction,
                        sql_query::<PendingRevisionRow>(
                            "select r.organization_id, r.project_id, r.environment_id, r.id, r.accepted_at from external_source_revisions r left join build_runs b on b.organization_id = r.organization_id and b.subject_kind = 'external_source_revision' and b.source_revision_id = r.id where b.id is null order by r.accepted_at asc, r.id asc limit ",
                        )
                        .bind(limit.max(1))
                        .append(" for update of r skip locked"),
                    )
                    .await?;
                    let releases = fetch_all::<PendingAssetReleaseRow, _>(
                        transaction,
                        sql_query::<PendingAssetReleaseRow>(
                            "select r.organization_id, r.asset_id, r.id, r.created_at from asset_releases r join assets a on a.organization_id = r.organization_id and a.id = r.asset_id left join build_runs b on b.organization_id = r.organization_id and b.subject_kind = 'asset_release' and b.asset_release_id = r.id where r.state = 'draft' and a.state = 'active' and a.kind in ('agent', 'mcp') and b.id is null order by r.created_at asc, r.id asc limit ",
                        )
                        .bind(limit.max(1))
                        .append(" for update of r skip locked"),
                    )
                    .await?;
                    let mut pending = revisions
                        .into_iter()
                        .map(PendingBuild::External)
                        .chain(releases.into_iter().map(PendingBuild::AssetRelease))
                        .collect::<Vec<_>>();
                    pending.sort_by_key(PendingBuild::sort_key);
                    let mut builds = Vec::with_capacity(limit.max(1).min(pending.len()));
                    for candidate in pending.into_iter().take(limit.max(1)) {
                        let build = match candidate {
                            PendingBuild::External((
                                organization_id,
                                project_id,
                                environment_id,
                                revision_id,
                                accepted_at,
                            )) => BuildRun::reserve(
                                OrganizationId::from_uuid(organization_id),
                                ProjectId::from_uuid(project_id),
                                EnvironmentId::from_uuid(environment_id),
                                SourceRevisionId::from_uuid(revision_id),
                                reserved_at.max(accepted_at),
                            ),
                            PendingBuild::AssetRelease((
                                organization_id,
                                asset_id,
                                asset_release_id,
                                drafted_at,
                            )) => BuildRun::reserve_asset_release(
                                OrganizationId::from_uuid(organization_id),
                                AssetId::from_uuid(asset_id),
                                AssetReleaseId::from_uuid(asset_release_id),
                                reserved_at.max(drafted_at),
                            ),
                        };
                        insert_build(transaction, &build).await?;
                        builds.push(build);
                    }
                    Ok(builds)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<BuildRunRow>(SELECT_BUILDS)
                    .append(
                        " left join operation_requests o on o.operation_id = b.operation_id where o.operation_id is null order by b.requested_at asc, b.id asc limit ",
                    )
                    .bind(limit.max(1)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .collect()
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<BuildRun, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<BuildRunRow>(SELECT_BUILDS)
                    .append(" where b.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and b.id = ")
                    .bind(build_run_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()?
            .ok_or(RepositoryError::NotFound)
    }

    async fn find_by_source_revision(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<BuildRunRow>(SELECT_BUILDS)
                    .append(" where b.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and b.source_revision_id = ")
                    .bind(source_revision_id.as_uuid())
                    .append(" order by b.attempt desc limit 1"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
    }

    async fn find_by_asset_release(
        &self,
        organization_id: OrganizationId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<BuildRunRow>(SELECT_BUILDS)
                    .append(" where b.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and b.asset_release_id = ")
                    .bind(asset_release_id.as_uuid())
                    .append(" order by b.attempt desc limit 1"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<BuildRunRow>(SELECT_BUILDS)
                    .append(" where b.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and b.project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and b.environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" order by b.requested_at desc, b.attempt desc, b.id desc limit ")
                    .bind(limit.max(1)),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .collect()
    }

    async fn request_cancellation(
        &self,
        request: RequestBuildCancellationBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<BuildRun>(transaction, &request.idempotency).await?
                    {
                        return Ok(IdempotentWrite {
                            value: replay.value,
                            replayed: true,
                        });
                    }
                    let existing = find_build_for_update(
                        transaction,
                        request.build_run.organization_id,
                        request.build_run.id,
                    )
                    .await?;
                    validate_build_run_transition(
                        &existing,
                        &request.build_run,
                        request.expected_version,
                    )
                    .map_err(PostgresPersistenceError::Repository)?;
                    let build_run =
                        persist_build(transaction, &request.build_run, request.expected_version)
                            .await?;
                    store_idempotency(transaction, &request.idempotency, &build_run).await?;
                    Ok(IdempotentWrite {
                        value: build_run,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    Ok(idempotency_replay::<BuildRun>(transaction, &idempotency)
                        .await?
                        .map(|replay| replay.value))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn request_retry(
        &self,
        request: RequestBuildRetryBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<BuildRun>(transaction, &request.idempotency).await?
                    {
                        return Ok(IdempotentWrite {
                            value: replay.value,
                            replayed: true,
                        });
                    }
                    let previous_id = request.retry.retry_of_build_run_id.ok_or_else(|| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(
                            "build retry has no parent".into(),
                        ))
                    })?;
                    let previous = find_build_for_update(
                        transaction,
                        request.retry.organization_id,
                        previous_id,
                    )
                    .await?;
                    validate_build_run_retry(
                        &previous,
                        &request.retry,
                        request.expected_previous_version,
                    )
                    .map_err(PostgresPersistenceError::Repository)?;
                    let existing_retry = fetch_optional::<BuildRunRow, _>(
                        transaction,
                        sql_query::<BuildRunRow>(SELECT_BUILDS)
                            .append(" where b.organization_id = ")
                            .bind(request.retry.organization_id.as_uuid())
                            .append(" and b.retry_of_build_run_id = ")
                            .bind(previous_id.as_uuid())
                            .append(" for update"),
                    )
                    .await?;
                    if existing_retry.is_some() {
                        return Err(PostgresPersistenceError::Repository(
                            RepositoryError::Conflict(
                                "build run already has a retry attempt".into(),
                            ),
                        ));
                    }
                    insert_build(transaction, &request.retry).await?;
                    store_idempotency(transaction, &request.idempotency, &request.retry).await?;
                    Ok(IdempotentWrite {
                        value: request.retry,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_retry(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    Ok(idempotency_replay::<BuildRun>(transaction, &idempotency)
                        .await?
                        .map(|replay| replay.value))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn save(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRun, RepositoryError> {
        let build_run = BuildRun::restore(build_run).map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing =
                        find_build_for_update(transaction, build_run.organization_id, build_run.id)
                            .await?;
                    validate_build_run_transition(&existing, &build_run, expected_version)
                        .map_err(PostgresPersistenceError::Repository)?;
                    persist_build(transaction, &build_run, expected_version).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn finalize(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRunFinalization, RepositoryError> {
        let build_run = BuildRun::restore(build_run).map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing = find_build_for_update(
                        transaction,
                        build_run.organization_id,
                        build_run.id,
                    )
                    .await?;
                    let mode = validate_build_run_finalization(
                        &existing,
                        &build_run,
                        expected_version,
                    )
                    .map_err(PostgresPersistenceError::Repository)?;

                    if build_run.asset_release_id().is_none() {
                        let completed = persist_finalized_build(
                            transaction,
                            existing,
                            build_run,
                            expected_version,
                            mode,
                        )
                        .await?;
                        return Ok(BuildRunFinalization::Completed(completed));
                    }

                    if build_run.status == BuildRunStatus::Succeeded {
                        let plan = plan_hosted_release(transaction, &build_run).await?;
                        return match plan {
                            HostedReleasePlan::Reject(reason) => {
                                if mode == BuildRunFinalizationMode::Replay {
                                    return Err(PostgresPersistenceError::Invariant(format!(
                                        "successful hosted BuildRun lost its release publication: {reason}"
                                    )));
                                }
                                let mut rejected = existing.clone();
                                rejected
                                    .record_failure(reason, build_run.updated_at)
                                    .map_err(|error| {
                                        PostgresPersistenceError::Invariant(format!(
                                            "hosted release rejection could not fail its BuildRun: {error}"
                                        ))
                                    })?;
                                validate_build_run_transition(
                                    &existing,
                                    &rejected,
                                    expected_version,
                                )
                                .map_err(PostgresPersistenceError::Repository)?;
                                let rejected =
                                    persist_build(transaction, &rejected, expected_version).await?;
                                Ok(BuildRunFinalization::Rejected(rejected))
                            }
                            plan => {
                                let completed = persist_finalized_build(
                                    transaction,
                                    existing,
                                    build_run,
                                    expected_version,
                                    mode,
                                )
                                .await?;
                                apply_hosted_release(transaction, plan).await?;
                                Ok(BuildRunFinalization::Completed(completed))
                            }
                        };
                    }

                    verify_hosted_release_unpublished(transaction, &build_run).await?;
                    let completed = persist_finalized_build(
                        transaction,
                        existing,
                        build_run,
                        expected_version,
                        mode,
                    )
                    .await?;
                    Ok(BuildRunFinalization::Completed(completed))
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn persist_finalized_build(
    transaction: &a3s_orm::PostgresTransaction,
    existing: BuildRun,
    build_run: BuildRun,
    expected_version: u64,
    mode: BuildRunFinalizationMode,
) -> Result<BuildRun, PostgresPersistenceError> {
    match mode {
        BuildRunFinalizationMode::Transition => {
            persist_build(transaction, &build_run, expected_version).await
        }
        BuildRunFinalizationMode::Replay => Ok(existing),
    }
}

async fn find_build_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    build_run_id: BuildRunId,
) -> Result<BuildRun, PostgresPersistenceError> {
    let row = fetch_optional::<BuildRunRow, _>(
        transaction,
        sql_query::<BuildRunRow>(SELECT_BUILDS)
            .append(" where b.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and b.id = ")
            .bind(build_run_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .ok_or(PostgresPersistenceError::Repository(
        RepositoryError::NotFound,
    ))?;
    map_row(row).map_err(PostgresPersistenceError::Repository)
}

async fn persist_build(
    transaction: &a3s_orm::PostgresTransaction,
    build_run: &BuildRun,
    expected_version: u64,
) -> Result<BuildRun, PostgresPersistenceError> {
    let input_artifact = json_value(build_run.input_artifact.as_ref())?;
    let box_build_output = json_value(build_run.box_build_output.as_ref())?;
    let output = json_value(build_run.output.as_ref())?;
    let publication_target = json_value(build_run.publication_target.as_ref())?;
    let published_artifact = json_value(build_run.published_artifact.as_ref())?;
    let evidence = json_value(build_run.evidence.as_ref())?;
    let updated = execute(
        transaction,
        sql_query::<()>("update build_runs set status = ")
            .bind(build_run.status.as_str())
            .append(", source_content_digest = ")
            .bind(build_run.source_content_digest.as_deref())
            .append(", input_artifact = ")
            .bind(input_artifact)
            .append(", node_id = ")
            .bind(build_run.node_id.map(NodeId::as_uuid))
            .append(", command_id = ")
            .bind(build_run.command_id.map(NodeCommandId::as_uuid))
            .append(", cleanup_command_id = ")
            .bind(build_run.cleanup_command_id.map(NodeCommandId::as_uuid))
            .append(", build_request_digest = ")
            .bind(build_run.build_request_digest.as_deref())
            .append(", box_build_output = ")
            .bind(box_build_output)
            .append(", output = ")
            .bind(output)
            .append(", publication_target = ")
            .bind(publication_target)
            .append(", published_artifact = ")
            .bind(published_artifact)
            .append(", evidence_required = ")
            .bind(build_run.evidence_required)
            .append(", evidence = ")
            .bind(evidence)
            .append(", failure = ")
            .bind(build_run.failure.as_deref())
            .append(", aggregate_version = ")
            .bind(build_run.aggregate_version)
            .append(", updated_at = ")
            .bind(build_run.updated_at)
            .append(", started_at = ")
            .bind(build_run.started_at)
            .append(", cancellation_requested_at = ")
            .bind(build_run.cancellation_requested_at)
            .append(", finished_at = ")
            .bind(build_run.finished_at)
            .append(" where organization_id = ")
            .bind(build_run.organization_id.as_uuid())
            .append(" and id = ")
            .bind(build_run.id.as_uuid())
            .append(" and aggregate_version = ")
            .bind(expected_version),
    )
    .await?;
    match updated {
        1 => {}
        0 => {
            let exists = fetch_optional::<i32, _>(
                transaction,
                sql_query::<i32>("select 1 from build_runs where organization_id = ")
                    .bind(build_run.organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(build_run.id.as_uuid()),
            )
            .await?
            .is_some();
            return Err(if exists {
                RepositoryError::Conflict("build run changed concurrently".into())
            } else {
                RepositoryError::NotFound
            }
            .into());
        }
        rows => {
            return Err(PostgresPersistenceError::Invariant(format!(
                "updating build run affected {rows} rows"
            )))
        }
    }
    let row = fetch_optional::<BuildRunRow, _>(
        transaction,
        sql_query::<BuildRunRow>(SELECT_BUILDS)
            .append(" where b.organization_id = ")
            .bind(build_run.organization_id.as_uuid())
            .append(" and b.id = ")
            .bind(build_run.id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("updated build run could not be reloaded".into())
    })?;
    map_row(row).map_err(PostgresPersistenceError::Repository)
}

async fn insert_build(
    transaction: &a3s_orm::PostgresTransaction,
    build: &BuildRun,
) -> Result<(), PostgresPersistenceError> {
    let (subject_kind, project_id, environment_id, source_revision_id, asset_id, asset_release_id) =
        match build.subject {
            BuildSubject::ExternalSourceRevision {
                project_id,
                environment_id,
                source_revision_id,
            } => (
                "external_source_revision",
                Some(project_id.as_uuid()),
                Some(environment_id.as_uuid()),
                Some(source_revision_id.as_uuid()),
                None,
                None,
            ),
            BuildSubject::AssetRelease {
                asset_id,
                asset_release_id,
            } => (
                "asset_release",
                None,
                None,
                None,
                Some(asset_id.as_uuid()),
                Some(asset_release_id.as_uuid()),
            ),
        };
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into build_runs (organization_id, subject_kind, project_id, environment_id, source_revision_id, asset_id, asset_release_id, id, attempt, retry_of_build_run_id, operation_id, status, evidence_required, aggregate_version, requested_at, updated_at) values (",
        )
        .bind(build.organization_id.as_uuid())
        .append(", ")
        .bind(subject_kind)
        .append(", ")
        .bind(project_id)
        .append(", ")
        .bind(environment_id)
        .append(", ")
        .bind(source_revision_id)
        .append(", ")
        .bind(asset_id)
        .append(", ")
        .bind(asset_release_id)
        .append(", ")
        .bind(build.id.as_uuid())
        .append(", ")
        .bind(build.attempt)
        .append(", ")
        .bind(build.retry_of_build_run_id.map(BuildRunId::as_uuid))
        .append(", ")
        .bind(build.operation_id.as_uuid())
        .append(", ")
        .bind(build.status.as_str())
        .append(", ")
        .bind(build.evidence_required)
        .append(", ")
        .bind(build.aggregate_version)
        .append(", ")
        .bind(build.requested_at)
        .append(", ")
        .bind(build.updated_at)
        .append(")"),
    )
    .await?;
    if inserted != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "reserving build run affected {inserted} rows"
        )));
    }
    Ok(())
}

fn json_value<T: serde::Serialize>(value: Option<&T>) -> Result<Option<Value>, serde_json::Error> {
    value.map(serde_json::to_value).transpose()
}

struct BuildRunRow {
    organization_id: Uuid,
    subject_kind: String,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    source_revision_id: Option<Uuid>,
    asset_id: Option<Uuid>,
    asset_release_id: Option<Uuid>,
    id: Uuid,
    attempt: u32,
    retry_of_build_run_id: Option<Uuid>,
    operation_id: Uuid,
    status: String,
    source_content_digest: Option<String>,
    input_artifact: Option<Value>,
    node_id: Option<Uuid>,
    command_id: Option<Uuid>,
    cleanup_command_id: Option<Uuid>,
    build_request_digest: Option<String>,
    box_build_output: Option<Value>,
    output: Option<Value>,
    publication_target: Option<Value>,
    published_artifact: Option<Value>,
    evidence_required: bool,
    evidence: Option<Value>,
    failure: Option<String>,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    cancellation_requested_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl FromRow for BuildRunRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            subject_kind: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            source_revision_id: decode(row, 4)?,
            asset_id: decode(row, 5)?,
            asset_release_id: decode(row, 6)?,
            id: decode(row, 7)?,
            attempt: decode(row, 8)?,
            retry_of_build_run_id: decode(row, 9)?,
            operation_id: decode(row, 10)?,
            status: decode(row, 11)?,
            source_content_digest: decode(row, 12)?,
            input_artifact: decode(row, 13)?,
            node_id: decode(row, 14)?,
            command_id: decode(row, 15)?,
            cleanup_command_id: decode(row, 16)?,
            build_request_digest: decode(row, 17)?,
            box_build_output: decode(row, 18)?,
            output: decode(row, 19)?,
            publication_target: decode(row, 20)?,
            published_artifact: decode(row, 21)?,
            evidence_required: decode(row, 22)?,
            evidence: decode(row, 23)?,
            failure: decode(row, 24)?,
            aggregate_version: decode(row, 25)?,
            requested_at: decode(row, 26)?,
            updated_at: decode(row, 27)?,
            started_at: decode(row, 28)?,
            cancellation_requested_at: decode(row, 29)?,
            finished_at: decode(row, 30)?,
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

fn map_row(row: BuildRunRow) -> Result<BuildRun, RepositoryError> {
    let input_artifact = decode_json::<BuildArtifact>(row.input_artifact, "input artifact")?;
    let box_build_output =
        decode_json::<NodeBoxBuildOutput>(row.box_build_output, "Box build output receipt")?;
    let output = decode_json::<ValidatedOciBuildOutput>(row.output, "validated output")?;
    let publication_target =
        decode_json::<OciPublicationTarget>(row.publication_target, "publication target")?;
    let published_artifact =
        decode_json::<PublishedOciArtifact>(row.published_artifact, "published artifact")?;
    let evidence =
        decode_json::<BuildEvidence>(row.evidence, "supply-chain evidence")?.map(Box::new);
    let subject = match (
        row.subject_kind.as_str(),
        row.project_id,
        row.environment_id,
        row.source_revision_id,
        row.asset_id,
        row.asset_release_id,
    ) {
        (
            "external_source_revision",
            Some(project_id),
            Some(environment_id),
            Some(source_revision_id),
            None,
            None,
        ) => BuildSubject::external_source_revision(
            ProjectId::from_uuid(project_id),
            EnvironmentId::from_uuid(environment_id),
            SourceRevisionId::from_uuid(source_revision_id),
        ),
        ("asset_release", None, None, None, Some(asset_id), Some(asset_release_id)) => {
            BuildSubject::asset_release(
                AssetId::from_uuid(asset_id),
                AssetReleaseId::from_uuid(asset_release_id),
            )
        }
        _ => return Err(corrupt("stored build subject shape is invalid")),
    };
    BuildRun::restore(BuildRun {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        subject,
        id: BuildRunId::from_uuid(row.id),
        attempt: row.attempt,
        retry_of_build_run_id: row.retry_of_build_run_id.map(BuildRunId::from_uuid),
        operation_id: OperationId::from_uuid(row.operation_id),
        status: BuildRunStatus::parse(&row.status)
            .map_err(|error| corrupt(format!("build status is invalid: {error}")))?,
        source_content_digest: row.source_content_digest,
        input_artifact,
        node_id: row.node_id.map(NodeId::from_uuid),
        command_id: row.command_id.map(NodeCommandId::from_uuid),
        cleanup_command_id: row.cleanup_command_id.map(NodeCommandId::from_uuid),
        build_request_digest: row.build_request_digest,
        box_build_output,
        output,
        publication_target,
        published_artifact,
        evidence_required: row.evidence_required,
        evidence,
        failure: row.failure,
        aggregate_version: row.aggregate_version,
        requested_at: row.requested_at,
        updated_at: row.updated_at,
        started_at: row.started_at,
        cancellation_requested_at: row.cancellation_requested_at,
        finished_at: row.finished_at,
    })
    .map_err(|error| corrupt(format!("stored build run is invalid: {error}")))
}

fn decode_json<T: serde::de::DeserializeOwned>(
    value: Option<Value>,
    label: &str,
) -> Result<Option<T>, RepositoryError> {
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| corrupt(format!("stored build {label} is invalid: {error}")))
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(message.into())
}
