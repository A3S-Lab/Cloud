use super::postgres_fixture::{get_as, post_json, response_json, ADMIN_TOKEN};
use a3s_cloud_contracts::{artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_cloud_control_plane::modules::artifacts::application::{
    BuildRunReconciler, BUILD_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::artifacts::domain::{
    RequestBuildCancellationBundle, RequestBuildRetryBundle,
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
    EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId, OrganizationId,
    ProjectId, RepositoryError, SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::domain::BuildPlatform;
use a3s_cloud_control_plane::modules::workloads::{
    IWorkloadRepository, PostgresWorkloadRepository,
};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

pub async fn exercise_build_run_persistence(
    app: &ControlPlane,
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
            .list(organization_id, project_id, environment_id, 1)
            .await?
            .as_slice(),
        reserved.as_slice()
    );

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
        .list(organization_id, project_id, EnvironmentId::new(), 100)
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

    let published = succeeded
        .published_artifact
        .as_ref()
        .ok_or("succeeded build omitted its published artifact")?;
    let source_workload_path = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions/{source_revision_id}/workloads"
    );
    let workload_body = |name: &str| {
        serde_json::json!({
            "name": name,
            "template": {
                "process": {},
                "secrets": [],
                "resources": {
                    "cpuMillis": 100,
                    "memoryBytes": 33554432,
                    "pids": 32,
                    "ephemeralStorageBytes": null
                },
                "ports": [{"name": "http", "containerPort": 8080}],
                "health": {
                    "portName": "http",
                    "path": "/health",
                    "intervalMs": 1000,
                    "timeoutMs": 500,
                    "healthyThreshold": 1,
                    "unhealthyThreshold": 3,
                    "stabilizationWindowMs": 1000
                }
            }
        })
    };
    let accepted = app
        .call(post_json(
            &source_workload_path,
            "source-build-workload",
            workload_body("source-build-api"),
        ))
        .await?;
    let replayed = app
        .call(post_json(
            &source_workload_path,
            "source-build-workload",
            workload_body("source-build-api"),
        ))
        .await?;
    assert_eq!(accepted.status(), 202);
    assert_eq!(replayed.status(), 200);
    let accepted_body = response_json(&accepted)?;
    let replayed_body = response_json(&replayed)?;
    assert_eq!(
        accepted_body["data"]["externalSourceRevisionId"],
        source_revision_id.to_string()
    );
    assert_eq!(accepted_body["data"]["buildRunId"], build_id.to_string());
    assert_eq!(accepted_body["data"]["artifactSourceUri"], published.uri);
    assert_eq!(
        accepted_body["data"]["expectedArtifactDigest"],
        published.digest
    );
    assert_eq!(
        accepted_body["data"]["deploymentId"],
        replayed_body["data"]["deploymentId"]
    );
    assert_eq!(replayed_body["data"]["replayed"], true);
    let changed = app
        .call(post_json(
            &source_workload_path,
            "source-build-workload",
            workload_body("source-build-changed"),
        ))
        .await?;
    assert_eq!(changed.status(), 409);

    let workload_id = Uuid::parse_str(
        accepted_body["data"]["workloadId"]
            .as_str()
            .ok_or("source-build response omitted workload ID")?,
    )?;
    let revision_id = Uuid::parse_str(
        accepted_body["data"]["revisionId"]
            .as_str()
            .ok_or("source-build response omitted revision ID")?,
    )?;
    let deployment_id = Uuid::parse_str(
        accepted_body["data"]["deploymentId"]
            .as_str()
            .ok_or("source-build response omitted deployment ID")?,
    )?;
    let deployment_operation_id = Uuid::parse_str(
        accepted_body["data"]["operationId"]
            .as_str()
            .ok_or("source-build response omitted operation ID")?,
    )?;
    let stored_trace = database
        .fetch_one_as(
            sql_query::<(Uuid, Uuid, Uuid, Uuid, Uuid)>(
                "select external_build_organization_id, external_build_project_id, external_build_environment_id, external_source_revision_id, external_build_run_id from workload_revisions where id = ",
            )
            .bind(revision_id),
        )
        .await?;
    assert_eq!(
        stored_trace,
        (
            organization_id.as_uuid(),
            project_id.as_uuid(),
            environment_id.as_uuid(),
            source_revision_id.as_uuid(),
            build_id.as_uuid(),
        )
    );
    let workload_repository = PostgresWorkloadRepository::new(executor.clone());
    let reconstructed = workload_repository
        .find_revision(
            organization_id,
            a3s_cloud_control_plane::modules::shared_kernel::domain::WorkloadRevisionId::from_uuid(
                revision_id,
            ),
        )
        .await?;
    let reconstructed_trace = reconstructed
        .external_build
        .ok_or("reconstructed revision omitted external build trace")?;
    assert_eq!(reconstructed_trace.source_revision_id, source_revision_id);
    assert_eq!(reconstructed_trace.build_run_id, build_id);
    let operation_input = database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select input from operation_requests where operation_id = ",
            )
            .bind(deployment_operation_id),
        )
        .await?;
    assert_eq!(
        operation_input["externalSourceRevisionId"],
        source_revision_id.to_string()
    );
    assert_eq!(operation_input["buildRunId"], build_id.to_string());
    let detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization_id}/workloads/{workload_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(detail.status(), 200);
    let detail = response_json(&detail)?;
    assert_eq!(
        detail["data"]["desiredRevision"]["externalSourceRevisionId"],
        source_revision_id.to_string()
    );
    assert_eq!(
        detail["data"]["desiredRevision"]["buildRunId"],
        build_id.to_string()
    );

    let deployment_operations_before_failure = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from operation_requests where workflow_name = 'cloud.deployment' and input ->> 'buildRunId' = ",
            )
            .bind(build_id.to_string()),
        )
        .await?;
    let outbox_events_before_failure = database
        .fetch_one_as(sql_query::<i64>("select count(*) from outbox_events"))
        .await?;

    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_external_build_deployment() returns trigger language plpgsql as $$
               begin
                 raise exception 'injected external build deployment failure';
               end
             $$;
             create trigger reject_external_build_deployment before insert on deployments
               for each row execute function reject_external_build_deployment();",
        )
        .await?;
    let rejected = app
        .call(post_json(
            &source_workload_path,
            "source-build-atomic-failure",
            workload_body("source-build-atomic-failure"),
        ))
        .await;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop trigger reject_external_build_deployment on deployments;
             drop function reject_external_build_deployment();",
        )
        .await?;
    let rejected = rejected?;
    assert_eq!(rejected.status(), 500);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from workloads where organization_id = ",)
                    .bind(organization_id.as_uuid())
                    .append(" and name_key = 'source-build-atomic-failure'"),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from workload_revisions where external_build_run_id = ",
                )
                .bind(build_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from idempotency_records where idempotency_key = 'source-build-atomic-failure'",
                ),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from operation_requests where workflow_name = 'cloud.deployment' and input ->> 'buildRunId' = ",
                )
                .bind(build_id.to_string()),
            )
            .await?,
        deployment_operations_before_failure
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>("select count(*) from outbox_events"))
            .await?,
        outbox_events_before_failure
    );

    database
        .execute(
            sql_query::<()>("delete from idempotency_records where idempotency_key = ")
                .bind("source-build-workload"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from outbox_events where aggregate_id = ").bind(deployment_id),
        )
        .await?;
    database
        .execute(sql_query::<()>("delete from deployments where id = ").bind(deployment_id))
        .await?;
    database
        .execute(
            sql_query::<()>("delete from operation_requests where operation_id = ")
                .bind(deployment_operation_id),
        )
        .await?;
    database
        .execute(sql_query::<()>("delete from workload_revisions where id = ").bind(revision_id))
        .await?;
    database
        .execute(sql_query::<()>("delete from workloads where id = ").bind(workload_id))
        .await?;

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
    let cancellation_source_revision_id = SourceRevisionId::new();
    let cancellation_accepted_at = chrono::Utc::now();
    let inserted = database
        .execute(
            sql_query::<()>(
                "insert into external_source_revisions (organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at) select organization_id, project_id, environment_id, ",
            )
            .bind(cancellation_source_revision_id.as_uuid())
            .append(", repository_provider, repository_url, repository_identity, ")
            .bind("1111111111111111111111111111111111111111")
            .append(", recipe, recipe_digest, 1, ")
            .bind(cancellation_accepted_at)
            .append(" from external_source_revisions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(source_revision_id.as_uuid()),
        )
        .await?;
    assert_eq!(inserted.rows_affected, 1);
    let queued_for_cancellation = builds
        .reserve_pending(1, cancellation_accepted_at)
        .await?
        .pop()
        .ok_or("cancellation build was not reserved")?;
    assert_eq!(
        queued_for_cancellation.id,
        BuildRun::id_for(cancellation_source_revision_id)
    );
    let mut cancelling = queued_for_cancellation.clone();
    cancelling.request_cancellation(cancellation_accepted_at + Duration::milliseconds(1))?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/build-runs/{}/cancellation",
            queued_for_cancellation.id
        ),
        "postgres-cancel-build",
        queued_for_cancellation.id.to_string().as_bytes(),
    )?;
    let cancellation = RequestBuildCancellationBundle {
        build_run: cancelling.clone(),
        expected_version: queued_for_cancellation.aggregate_version,
        idempotency: idempotency.clone(),
    };
    let (left, right) = tokio::join!(
        builds.request_cancellation(cancellation.clone()),
        builds.request_cancellation(cancellation),
    );
    let cancellations = [left?, right?];
    assert_eq!(
        cancellations
            .iter()
            .filter(|result| result.replayed)
            .count(),
        1
    );
    assert!(cancellations
        .iter()
        .all(|result| result.value == cancelling));
    assert_eq!(
        builds.replay_cancellation(&idempotency).await?,
        Some(cancelling.clone())
    );
    let conflicting_idempotency = IdempotencyRequest::new(
        idempotency.scope.clone(),
        idempotency.key.clone(),
        b"different cancellation",
    )?;
    assert_eq!(
        builds.replay_cancellation(&conflicting_idempotency).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        builds
            .find(organization_id, queued_for_cancellation.id)
            .await?,
        cancelling
    );
    let mut cancelled = cancelling.clone();
    cancelled.complete(cancellation_accepted_at + Duration::milliseconds(2))?;
    let cancelled = builds.save(cancelled, cancelling.aggregate_version).await?;
    let retry = BuildRun::retry(
        &cancelled,
        cancellation_accepted_at + Duration::milliseconds(3),
    )?;
    let retry_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/build-runs/{}/retry",
            cancelled.id
        ),
        "postgres-retry-build",
        cancelled.id.to_string().as_bytes(),
    )?;
    let retry_request = RequestBuildRetryBundle {
        retry: retry.clone(),
        expected_previous_version: cancelled.aggregate_version,
        idempotency: retry_idempotency.clone(),
    };
    let (left, right) = tokio::join!(
        builds.request_retry(retry_request.clone()),
        builds.request_retry(retry_request),
    );
    let retries = [left?, right?];
    assert_eq!(retries.iter().filter(|result| result.replayed).count(), 1);
    assert!(retries.iter().all(|result| result.value == retry));
    assert_eq!(
        builds.replay_retry(&retry_idempotency).await?,
        Some(retry.clone())
    );
    assert_eq!(
        builds
            .find_by_source_revision(organization_id, cancellation_source_revision_id)
            .await?,
        Some(retry.clone())
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from build_runs where organization_id = ",)
                    .bind(organization_id.as_uuid())
                    .append(" and source_revision_id = ")
                    .bind(cancellation_source_revision_id.as_uuid()),
            )
            .await?,
        2
    );
    let conflicting_retry_idempotency = IdempotencyRequest::new(
        retry_idempotency.scope.clone(),
        retry_idempotency.key.clone(),
        b"different retry",
    )?;
    assert_eq!(
        builds.replay_retry(&conflicting_retry_idempotency).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    let duplicate_retry = RequestBuildRetryBundle {
        retry: retry.clone(),
        expected_previous_version: cancelled.aggregate_version,
        idempotency: IdempotencyRequest::new(
            retry_idempotency.scope.clone(),
            "postgres-retry-build-again",
            cancelled.id.to_string().as_bytes(),
        )?,
    };
    assert!(matches!(
        builds.request_retry(duplicate_retry).await,
        Err(RepositoryError::Conflict(_))
    ));
    database
        .execute(
            sql_query::<()>("delete from idempotency_records where scope_key = ")
                .bind(retry_idempotency.scope)
                .append(" and idempotency_key = ")
                .bind(retry_idempotency.key),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from build_runs where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(retry.id.as_uuid()),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from idempotency_records where scope_key = ")
                .bind(idempotency.scope)
                .append(" and idempotency_key = ")
                .bind(idempotency.key),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from build_runs where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(queued_for_cancellation.id.as_uuid()),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("delete from external_source_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(cancellation_source_revision_id.as_uuid()),
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
