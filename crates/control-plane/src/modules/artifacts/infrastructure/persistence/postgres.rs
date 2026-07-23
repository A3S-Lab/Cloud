use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, store_idempotency, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::artifacts::domain::repositories::{
    validate_build_run_retry, validate_build_run_transition,
};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildRun, BuildRunStatus, IBuildRunRepository, OciPublicationTarget,
    PublishedOciArtifact, RequestBuildCancellationBundle, RequestBuildRetryBundle,
    ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId,
    OperationId, OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_BUILDS: &str = "select b.organization_id, b.project_id, b.environment_id, b.id, b.source_revision_id, b.attempt, b.retry_of_build_run_id, b.operation_id, b.status, b.source_content_digest, b.input_artifact, b.node_id, b.command_id, b.cleanup_command_id, b.runtime_spec_digest, b.runtime_output_artifact, b.output, b.publication_target, b.published_artifact, b.failure, b.aggregate_version, b.requested_at, b.updated_at, b.started_at, b.cancellation_requested_at, b.finished_at from build_runs b";

type PendingRevisionRow = (Uuid, Uuid, Uuid, Uuid, DateTime<Utc>);

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
                            "select r.organization_id, r.project_id, r.environment_id, r.id, r.accepted_at from external_source_revisions r left join build_runs b on b.organization_id = r.organization_id and b.source_revision_id = r.id where b.id is null order by r.accepted_at asc, r.id asc limit ",
                        )
                        .bind(limit.max(1))
                        .append(" for update of r skip locked"),
                    )
                    .await?;
                    let mut builds = Vec::with_capacity(revisions.len());
                    for (organization_id, project_id, environment_id, revision_id, accepted_at) in
                        revisions
                    {
                        let build = BuildRun::reserve(
                            OrganizationId::from_uuid(organization_id),
                            ProjectId::from_uuid(project_id),
                            EnvironmentId::from_uuid(environment_id),
                            SourceRevisionId::from_uuid(revision_id),
                            reserved_at.max(accepted_at),
                        );
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
    let runtime_output_artifact = json_value(build_run.runtime_output_artifact.as_ref())?;
    let output = json_value(build_run.output.as_ref())?;
    let publication_target = json_value(build_run.publication_target.as_ref())?;
    let published_artifact = json_value(build_run.published_artifact.as_ref())?;
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
            .append(", runtime_spec_digest = ")
            .bind(build_run.runtime_spec_digest.as_deref())
            .append(", runtime_output_artifact = ")
            .bind(runtime_output_artifact)
            .append(", output = ")
            .bind(output)
            .append(", publication_target = ")
            .bind(publication_target)
            .append(", published_artifact = ")
            .bind(published_artifact)
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
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into build_runs (organization_id, project_id, environment_id, id, source_revision_id, attempt, retry_of_build_run_id, operation_id, status, aggregate_version, requested_at, updated_at) values (",
        )
        .bind(build.organization_id.as_uuid())
        .append(", ")
        .bind(build.project_id.as_uuid())
        .append(", ")
        .bind(build.environment_id.as_uuid())
        .append(", ")
        .bind(build.id.as_uuid())
        .append(", ")
        .bind(build.source_revision_id.as_uuid())
        .append(", ")
        .bind(build.attempt)
        .append(", ")
        .bind(build.retry_of_build_run_id.map(BuildRunId::as_uuid))
        .append(", ")
        .bind(build.operation_id.as_uuid())
        .append(", ")
        .bind(build.status.as_str())
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
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    source_revision_id: Uuid,
    attempt: u32,
    retry_of_build_run_id: Option<Uuid>,
    operation_id: Uuid,
    status: String,
    source_content_digest: Option<String>,
    input_artifact: Option<Value>,
    node_id: Option<Uuid>,
    command_id: Option<Uuid>,
    cleanup_command_id: Option<Uuid>,
    runtime_spec_digest: Option<String>,
    runtime_output_artifact: Option<Value>,
    output: Option<Value>,
    publication_target: Option<Value>,
    published_artifact: Option<Value>,
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
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            source_revision_id: decode(row, 4)?,
            attempt: decode(row, 5)?,
            retry_of_build_run_id: decode(row, 6)?,
            operation_id: decode(row, 7)?,
            status: decode(row, 8)?,
            source_content_digest: decode(row, 9)?,
            input_artifact: decode(row, 10)?,
            node_id: decode(row, 11)?,
            command_id: decode(row, 12)?,
            cleanup_command_id: decode(row, 13)?,
            runtime_spec_digest: decode(row, 14)?,
            runtime_output_artifact: decode(row, 15)?,
            output: decode(row, 16)?,
            publication_target: decode(row, 17)?,
            published_artifact: decode(row, 18)?,
            failure: decode(row, 19)?,
            aggregate_version: decode(row, 20)?,
            requested_at: decode(row, 21)?,
            updated_at: decode(row, 22)?,
            started_at: decode(row, 23)?,
            cancellation_requested_at: decode(row, 24)?,
            finished_at: decode(row, 25)?,
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
    let runtime_output_artifact =
        decode_json::<BuildArtifact>(row.runtime_output_artifact, "Runtime output artifact")?;
    let output = decode_json::<ValidatedOciBuildOutput>(row.output, "validated output")?;
    let publication_target =
        decode_json::<OciPublicationTarget>(row.publication_target, "publication target")?;
    let published_artifact =
        decode_json::<PublishedOciArtifact>(row.published_artifact, "published artifact")?;
    BuildRun::restore(BuildRun {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        id: BuildRunId::from_uuid(row.id),
        source_revision_id: SourceRevisionId::from_uuid(row.source_revision_id),
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
        runtime_spec_digest: row.runtime_spec_digest,
        runtime_output_artifact,
        output,
        publication_target,
        published_artifact,
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
