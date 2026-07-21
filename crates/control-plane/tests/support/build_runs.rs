use a3s_cloud_contracts::{artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_cloud_control_plane::modules::artifacts::application::{
    BuildRunReconciler, BUILD_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildRun, BuildRunStatus, IBuildRunRepository, OciDescriptor,
    OciPublicationTarget, PostgresBuildRunRepository, PublishedOciArtifact,
    ValidatedOciBuildOutput,
};
use a3s_cloud_control_plane::modules::operations::{
    IOperationRepository, PostgresOperationRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::domain::BuildPlatform;
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
    let database = Database::new(PostgresDialect, executor.clone());

    let (left, right) = tokio::join!(
        builds.reserve_pending(1, chrono::Utc::now()),
        builds.reserve_pending(1, chrono::Utc::now())
    );
    let mut reserved = left?;
    reserved.extend(right?);
    assert_eq!(reserved.len(), 1);
    let build_id = BuildRun::id_for(source_revision_id);
    let node_id = NodeId::new();
    let apply_command_id = NodeCommandId::new();
    let cleanup_command_id = NodeCommandId::new();
    let command_time = chrono::Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, aggregate_version) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", 'build publication fixture', 'build-publication-fixture', 'ready', ")
            .bind(Uuid::now_v7())
            .append(", 'test', 'test-runtime', 'test-runtime-1', ")
            .bind(format!("sha256:{}", "f".repeat(64)))
            .append(", ")
            .bind(serde_json::json!({}))
            .append(", ")
            .bind(command_time)
            .append(", ")
            .bind(command_time)
            .append(", 1)"),
        )
        .await?;
    for (command_id, sequence, kind) in [
        (apply_command_id, 1_i64, "runtime_apply"),
        (cleanup_command_id, 2_i64, "runtime_remove"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into node_commands (id, node_id, sequence, aggregate_id, generation, command_kind, payload_schema, payload_digest, payload, issued_at, not_after, correlation_id) values (",
                )
                .bind(command_id.as_uuid())
                .append(", ")
                .bind(node_id.as_uuid())
                .append(", ")
                .bind(sequence)
                .append(", ")
                .bind(build_id.as_uuid())
                .append(", 1, ")
                .bind(kind)
                .append(", 'test.command.v1', ")
                .bind(format!("sha256:{}", "9".repeat(64)))
                .append(", ")
                .bind(serde_json::json!({}))
                .append(", ")
                .bind(command_time)
                .append(", ")
                .bind(command_time + Duration::minutes(1))
                .append(", ")
                .bind(build_id.as_uuid())
                .append(")"),
            )
            .await?;
    }
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
    assert_eq!(operation.workflow.version(), BUILD_WORKFLOW_VERSION);
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

    let input_digest = format!("sha256:{}", "a".repeat(64));
    let input_artifact = build_artifact('b', 4_096)?;
    let mut prepared = preparing.clone();
    prepared.record_input(
        input_digest,
        input_artifact,
        preparing.updated_at + Duration::milliseconds(1),
    )?;
    let prepared = builds.save(prepared, preparing.aggregate_version).await?;
    let mut scheduled = prepared.clone();
    scheduled.schedule(
        node_id,
        format!("sha256:{}", "c".repeat(64)),
        prepared.updated_at + Duration::milliseconds(1),
    )?;
    let scheduled = builds.save(scheduled, prepared.aggregate_version).await?;
    let mut running = scheduled.clone();
    running.dispatch(
        apply_command_id,
        scheduled.updated_at + Duration::milliseconds(1),
    )?;
    let running = builds.save(running, scheduled.aggregate_version).await?;
    let runtime_output = build_artifact('d', 8_192)?;
    let mut validating = running.clone();
    validating.begin_validation(
        runtime_output.clone(),
        running.updated_at + Duration::milliseconds(1),
    )?;
    let validating = builds.save(validating, running.aggregate_version).await?;
    let descriptor = OciDescriptor::new(
        "application/vnd.oci.image.manifest.v1+json",
        format!("sha256:{}", "e".repeat(64)),
        512,
    )?;
    let output = ValidatedOciBuildOutput {
        artifact: runtime_output,
        descriptor: descriptor.clone(),
        platforms: vec![BuildPlatform::parse("linux/amd64")?],
        content_bytes: 2_048,
        blob_count: 3,
    };
    let mut validated = validating.clone();
    validated.record_validated_output(output, validating.updated_at + Duration::milliseconds(1))?;
    let validated = builds.save(validated, validating.aggregate_version).await?;
    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s/builds/{build_id}"),
        descriptor,
    )?;
    let mut combined_publication = validated.clone();
    combined_publication.begin_publication(
        target.clone(),
        validated.updated_at + Duration::milliseconds(1),
    )?;
    combined_publication.record_published_artifact(
        PublishedOciArtifact::from_target(&target),
        validated.updated_at + Duration::milliseconds(2),
    )?;
    assert!(matches!(
        builds
            .save(combined_publication, validated.aggregate_version)
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let mut publishing = validated.clone();
    publishing.begin_publication(
        target.clone(),
        validated.updated_at + Duration::milliseconds(1),
    )?;
    let publishing = builds.save(publishing, validated.aggregate_version).await?;
    let mut published = publishing.clone();
    published.record_published_artifact(
        PublishedOciArtifact::from_target(&target),
        publishing.updated_at + Duration::milliseconds(1),
    )?;
    let published = builds.save(published, publishing.aggregate_version).await?;
    let mut cleaning = published.clone();
    cleaning.begin_cleanup(
        cleanup_command_id,
        published.updated_at + Duration::milliseconds(1),
    )?;
    let cleaning = builds.save(cleaning, published.aggregate_version).await?;
    let mut succeeded = cleaning.clone();
    succeeded.complete(cleaning.updated_at + Duration::milliseconds(1))?;
    let succeeded = builds.save(succeeded, cleaning.aggregate_version).await?;
    assert_eq!(succeeded.status, BuildRunStatus::Succeeded);
    assert_eq!(builds.find(organization_id, build_id).await?, succeeded);

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
    database
        .execute(
            sql_query::<()>("delete from node_commands where node_id = ").bind(node_id.as_uuid()),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from nodes where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(node_id.as_uuid()),
        )
        .await?;
    Ok(())
}

fn build_artifact(
    digest_character: char,
    size_bytes: u64,
) -> Result<BuildArtifact, Box<dyn std::error::Error>> {
    let digest = format!("sha256:{}", digest_character.to_string().repeat(64));
    Ok(BuildArtifact::new(
        artifact_uri(&digest)?,
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        size_bytes,
    )?)
}
