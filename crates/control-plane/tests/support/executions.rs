use a3s_cloud_control_plane::modules::executions::domain::events::{
    ExecutionCancellationRequested, ExecutionRequested,
};
use a3s_cloud_control_plane::modules::executions::domain::{
    CreateExecution, Execution, ExecutionArtifact, ExecutionOutcome, ExecutionProcess,
    ExecutionResources, ExecutionStatus, ExecutionTemplate, IExecutionRepository,
    TransitionExecution,
};
use a3s_cloud_control_plane::modules::executions::PostgresExecutionRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn exercise_execution_persistence(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> TestResult {
    let repository = PostgresExecutionRepository::new(executor.clone());
    let execution = execution(organization_id, project_id, environment_id, Utc::now())?;
    let create = CreateExecution {
        execution: execution.clone(),
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"
            ),
            "postgres-execution-create",
            b"postgres-execution-request",
        )?,
        event: ExecutionRequested::envelope(&execution, Uuid::now_v7())?,
    };
    let (left, right) = tokio::join!(
        repository.create(create.clone()),
        repository.create(create.clone()),
    );
    let created = [left?, right?];
    assert_eq!(created.iter().filter(|write| write.replayed).count(), 1);
    assert!(created.iter().all(|write| write.execution == execution));

    let mut conflicting = create;
    conflicting.idempotency = IdempotencyRequest::new(
        conflicting.idempotency.scope.clone(),
        conflicting.idempotency.key.clone(),
        b"changed postgres execution request",
    )?;
    assert_eq!(
        repository.create(conflicting).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        repository.find(organization_id, execution.id).await?,
        Some(execution.clone())
    );
    assert_eq!(
        repository.find(other_organization_id, execution.id).await?,
        None
    );
    assert_eq!(
        repository
            .list(organization_id, project_id, environment_id, 10)
            .await?,
        vec![execution.clone()]
    );
    assert!(repository
        .list(other_organization_id, project_id, environment_id, 10)
        .await?
        .is_empty());
    assert!(repository
        .pending_operation_starts(100)
        .await?
        .contains(&execution));

    let mut cancelling = execution.clone();
    let expected_version = cancelling.aggregate_version;
    cancelling.request_cancellation(execution.updated_at + Duration::milliseconds(1))?;
    let cancellation = TransitionExecution {
        execution: cancelling.clone(),
        expected_version,
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{organization_id}/executions/{}/cancellation",
                execution.id
            ),
            "postgres-execution-cancel",
            b"postgres-execution-cancellation",
        )?,
        event: ExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())?,
    };
    let (left, right) = tokio::join!(
        repository.request_cancellation(cancellation.clone()),
        repository.request_cancellation(cancellation),
    );
    let cancelled = [left?, right?];
    assert_eq!(cancelled.iter().filter(|write| write.replayed).count(), 1);
    assert!(cancelled.iter().all(|write| write.execution == cancelling));

    let expected_version = cancelling.aggregate_version;
    cancelling.begin_cleanup(
        ExecutionOutcome::Cancelled,
        cancelling.updated_at + Duration::milliseconds(1),
    )?;
    let mut cleaning = repository.save(cancelling, expected_version).await?;
    assert_eq!(cleaning.status, ExecutionStatus::CleanupPending);
    let expected_version = cleaning.aggregate_version;
    cleaning.complete_cleanup(cleaning.updated_at + Duration::milliseconds(1))?;
    let completed = repository.save(cleaning, expected_version).await?;
    assert_eq!(completed.status, ExecutionStatus::Cancelled);
    assert!(completed.finished_at.is_some());
    assert!(!repository
        .pending_operation_starts(100)
        .await?
        .contains(&completed));

    let database = Database::new(PostgresDialect, executor.clone());
    let events = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where aggregate_id = ",
            )
            .bind(execution.id.as_uuid())
            .append(
                " and event_key in ('execution.run.requested', 'execution.run.cancellation-requested')",
            ),
        )
        .await?;
    assert_eq!(events, 2);
    Ok(())
}

fn execution(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    requested_at: chrono::DateTime<Utc>,
) -> Result<Execution, String> {
    let digest = format!("sha256:{}", "d".repeat(64));
    Execution::create(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/tasks/postgres@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/app/task".into()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            input: serde_json::json!({"fixture": "postgres"}),
            resources: ExecutionResources {
                cpu_millis: 250,
                memory_bytes: 128 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: None,
                timeout_ms: 5_000,
            },
        },
        requested_at,
    )
}
