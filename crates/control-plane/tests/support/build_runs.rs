use a3s_cloud_control_plane::modules::artifacts::application::BuildRunReconciler;
use a3s_cloud_control_plane::modules::artifacts::{
    BuildRun, IBuildRunRepository, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::operations::{
    IOperationRepository, PostgresOperationRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, OperationId, OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

pub async fn exercise_build_run_persistence(
    executor: &PostgresExecutor,
    organization_id: &str,
    project_id: &str,
    environment_id: &str,
    source_revision_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(Uuid::parse_str(organization_id)?);
    let project_id = ProjectId::from_uuid(Uuid::parse_str(project_id)?);
    let environment_id = EnvironmentId::from_uuid(Uuid::parse_str(environment_id)?);
    let source_revision_id = SourceRevisionId::from_uuid(Uuid::parse_str(source_revision_id)?);
    let builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));

    let (left, right) = tokio::join!(
        builds.reserve_pending(1, chrono::Utc::now()),
        builds.reserve_pending(1, chrono::Utc::now())
    );
    let mut reserved = left?;
    reserved.extend(right?);
    assert_eq!(reserved.len(), 1);
    let build_id = BuildRun::id_for(source_revision_id);
    assert_eq!(reserved[0].id, build_id);
    assert_eq!(reserved[0].organization_id, organization_id);
    assert_eq!(reserved[0].project_id, project_id);
    assert_eq!(reserved[0].environment_id, environment_id);

    assert_eq!(
        builds
            .find_by_source_revision(organization_id, source_revision_id)
            .await?
            .as_ref()
            .map(|build| build.id),
        Some(build_id)
    );
    assert!(builds
        .find_by_source_revision(OrganizationId::new(), source_revision_id)
        .await?
        .is_none());
    assert!(matches!(
        builds.find(OrganizationId::new(), build_id).await,
        Err(RepositoryError::NotFound)
    ));
    assert!(builds
        .list(organization_id, project_id, EnvironmentId::new())
        .await?
        .is_empty());

    let pending = builds.pending_operation_starts(10).await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, build_id);

    let operations = Arc::new(PostgresOperationRepository::new(executor.clone()));
    let reconciler = BuildRunReconciler::new(builds.clone(), operations.clone());
    let repaired = reconciler.run_once(10).await?;
    assert_eq!(repaired.reserved, 0);
    assert_eq!(repaired.started, 1);
    assert_eq!(repaired.replayed, 0);
    assert!(repaired.failures.is_empty());

    let operation_id = OperationId::from_uuid(build_id.as_uuid());
    let operation = operations
        .find_request(operation_id)
        .await?
        .ok_or("build operation was not enqueued")?;
    assert_eq!(operation.organization_id, organization_id);
    assert_eq!(operation.subject.kind(), "build_run");
    assert_eq!(operation.subject.id(), build_id.as_uuid());
    assert_eq!(operation.workflow.name(), "cloud.build");
    assert_eq!(operation.workflow.version(), "1");
    assert_eq!(
        operation.input["buildRunId"],
        serde_json::Value::String(build_id.to_string())
    );
    let exact_replay = operations.enqueue(operation.clone()).await?;
    assert!(exact_replay.replayed);
    assert_eq!(exact_replay.value, operation);
    let settled = reconciler.run_once(10).await?;
    assert_eq!(settled.reserved, 0);
    assert_eq!(settled.started, 0);
    assert_eq!(settled.replayed, 0);
    assert!(settled.failures.is_empty());

    let queued = builds.find(organization_id, build_id).await?;
    let mut preparing = queued.clone();
    preparing.begin_preparation(queued.updated_at + Duration::milliseconds(1))?;
    let preparing = builds.save(preparing, queued.aggregate_version).await?;
    let mut stale = queued;
    stale.begin_preparation(preparing.updated_at + Duration::milliseconds(1))?;
    let stale_expected_version = stale.aggregate_version - 1;
    assert!(matches!(
        builds.save(stale, stale_expected_version).await,
        Err(RepositoryError::Conflict(_))
    ));

    let mut forged = preparing.clone();
    forged.project_id = ProjectId::new();
    forged.aggregate_version += 1;
    forged.updated_at += Duration::milliseconds(1);
    assert!(matches!(
        builds.save(forged, preparing.aggregate_version).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(builds.find(organization_id, build_id).await?, preparing);

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from build_runs where source_revision_id = ",)
                    .bind(source_revision_id.as_uuid())
            )
            .await?,
        1
    );
    database
        .execute(
            sql_query::<()>("delete from operation_requests where operation_id = ")
                .bind(operation_id.as_uuid()),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from build_runs where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
        )
        .await?;
    Ok(())
}
