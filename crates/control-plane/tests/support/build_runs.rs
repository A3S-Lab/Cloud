use super::postgres_fixture::{get_as, post_json, response_json, ADMIN_TOKEN};
use crate::build_evidence_support::evidence_for;
use a3s_cloud_contracts::{
    artifact_uri, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor,
    NodeBoxBuildOutput, NodeBoxBuildPlatform, BOX_BUILD_OUTPUT_NAME,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_cloud_control_plane::modules::artifacts::application::{
    BuildRunReconciler, BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::artifacts::domain::{
    RequestBuildCancellationBundle, RequestBuildRetryBundle,
};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildRun, BuildRunFinalization, BuildRunStatus, BuildSubject,
    IBuildRunRepository, OciDescriptor, OciPublicationTarget, PostgresBuildRunRepository,
    PublishedOciArtifact, ValidatedOciBuildOutput,
};
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetRelease, AssetReleaseDrafted, AssetReleaseState, AssetReleaseVersion,
    CreateAssetReleaseWrite, IAssetRepository, PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::operations::{
    IOperationRepository, OperationRequest, OperationSubject, PostgresOperationRepository,
    WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetReleaseId, BuildRunId, EnvironmentId, GitCommitSha, IdempotencyRequest, NodeCommandId,
    NodeId, OperationId, OrganizationId, ProjectId, RepositoryError, Sha256Digest,
    SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::domain::BuildPlatform;
use a3s_cloud_control_plane::modules::workloads::{
    IWorkloadRepository, PostgresWorkloadRepository,
};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
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
        (apply_command_id, 1_i64, "box_build_start"),
        (cleanup_command_id, 2_i64, "box_build_remove"),
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
    assert_eq!(reserved[0].project_id(), Some(project_id));
    assert_eq!(reserved[0].environment_id(), Some(environment_id));
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
    forged.subject = BuildSubject::external_source_revision(
        ProjectId::new(),
        forged.environment_id().expect("external environment"),
        forged
            .source_revision_id()
            .expect("external source revision"),
    );
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
        input_artifact.clone(),
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
        box_output(&runtime_output, &input_artifact)?,
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
    let published = Box::pin(attest_and_complete_published_build(
        &builds,
        executor,
        organization_id,
        build_id,
        published,
        cleanup_command_id,
    ))
    .await?;
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
    let cancelled = builds
        .finalize(cancelled, cancelling.aggregate_version)
        .await?;
    let BuildRunFinalization::Completed(cancelled) = cancelled else {
        return Err("external BuildRun finalization was unexpectedly rejected".into());
    };
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

pub async fn exercise_hosted_build_run_persistence(
    executor: &PostgresExecutor,
    asset: &Asset,
) -> Result<(), Box<dyn std::error::Error>> {
    let assets = PostgresAssetRepository::new(executor.clone());
    let release = AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0")?,
        GitCommitSha::parse("a".repeat(40))?,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))?,
        chrono::Utc::now().max(asset.updated_at),
    )?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/assets/{}/releases",
            asset.organization_id, asset.id
        ),
        "postgres-hosted-build-draft",
        release.id.as_uuid().as_bytes(),
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            event: AssetReleaseDrafted::envelope(&release, release.id.as_uuid())?,
            release: release.clone(),
            idempotency: idempotency.clone(),
        })
        .await?;

    // A draft can be committed before a reconciler process observes it. Two
    // restarted workers must repair that gap by reserving exactly one BuildRun.
    let left_repository = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let right_repository = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let reserved_at = chrono::Utc::now().max(release.created_at);
    let (left, right) = tokio::join!(
        left_repository.reserve_pending(1, reserved_at),
        right_repository.reserve_pending(1, reserved_at),
    );
    let mut reserved = left?;
    reserved.extend(right?);
    assert_eq!(reserved.len(), 1);
    let mut build = reserved.pop().ok_or("hosted BuildRun was not reserved")?;
    assert_eq!(build.organization_id, asset.organization_id);
    assert_eq!(build.asset_id(), Some(asset.id));
    assert_eq!(build.asset_release_id(), Some(release.id));
    assert_eq!(build.project_id(), None);
    assert_eq!(build.environment_id(), None);
    assert_eq!(build.source_revision_id(), None);
    assert_eq!(build.id, BuildRun::id_for_subject(build.subject));
    assert_eq!(
        left_repository
            .find_by_asset_release(asset.organization_id, release.id)
            .await?,
        Some(build.clone())
    );
    assert!(left_repository
        .find_by_asset_release(OrganizationId::new(), release.id)
        .await?
        .is_none());

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(String, Option<Uuid>, Option<Uuid>, Option<Uuid>, Uuid, Uuid)>(
                    "select subject_kind, project_id, environment_id, source_revision_id, asset_id, asset_release_id from build_runs where organization_id = ",
                )
                .bind(asset.organization_id.as_uuid())
                .append(" and id = ")
                .bind(build.id.as_uuid()),
            )
            .await?,
        (
            "asset_release".into(),
            None,
            None,
            None,
            asset.id.as_uuid(),
            release.id.as_uuid(),
        )
    );

    let queued_version = build.aggregate_version;
    build.begin_preparation(reserved_at + Duration::milliseconds(1))?;
    build = left_repository.save(build, queued_version).await?;
    assert_eq!(build.status, BuildRunStatus::Preparing);
    assert_eq!(build.asset_id(), Some(asset.id));
    assert_eq!(build.asset_release_id(), Some(release.id));

    let operations = Arc::new(PostgresOperationRepository::new(executor.clone()));
    let restarted = BuildRunReconciler::new(left_repository.clone(), operations.clone());
    let repaired = restarted.run_once(10).await?;
    assert_eq!(repaired.reserved, 0);
    assert_eq!(repaired.started, 1);
    assert_eq!(repaired.replayed, 0);
    assert!(repaired.failures.is_empty());
    let operation = operations
        .find_request(build.operation_id)
        .await?
        .ok_or("hosted build operation was not enqueued")?;
    assert_eq!(operation.organization_id, asset.organization_id);
    assert_eq!(operation.workflow.name(), "cloud.build");
    assert_eq!(operation.workflow.version(), BUILD_WORKFLOW_VERSION);
    assert_eq!(
        operation.input["buildRunId"],
        serde_json::Value::String(build.id.to_string())
    );
    assert_eq!(
        assets
            .find_release(asset.organization_id, asset.id, release.id)
            .await?
            .map(|release| release.state),
        Some(AssetReleaseState::Draft)
    );

    let publication_events = || {
        database.fetch_one_as(
            sql_query::<i64>("select count(*) from outbox_events where organization_id = ")
                .bind(asset.organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(release.id.as_uuid())
                .append(" and event_key = 'asset.release.published'"),
        )
    };
    assert_eq!(publication_events().await?, 0);

    // A terminal hosted failure keeps the immutable release draft recoverable.
    // Recovery uses the generic BuildRun retry authority instead of creating an
    // Asset-specific queue, worker, or state machine.
    let first_attempt_id = build.id;
    let expected = build.aggregate_version;
    build.record_failure(
        "injected hosted builder failure".into(),
        build.updated_at + Duration::milliseconds(1),
    )?;
    build = left_repository.save(build, expected).await?;
    let expected = build.aggregate_version;
    build.complete(build.updated_at + Duration::milliseconds(1))?;
    assert!(matches!(
        left_repository.save(build.clone(), expected).await,
        Err(RepositoryError::Conflict(_))
    ));
    let failed = left_repository.finalize(build, expected).await?;
    let BuildRunFinalization::Completed(failed) = failed else {
        return Err("failed hosted Asset build was unexpectedly rejected".into());
    };
    assert_eq!(failed.status, BuildRunStatus::Failed);
    assert_eq!(
        assets
            .find_release(asset.organization_id, asset.id, release.id)
            .await?,
        Some(release.clone())
    );
    assert_eq!(publication_events().await?, 0);

    let retry = BuildRun::retry(&failed, failed.updated_at + Duration::milliseconds(1))?;
    let retry_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/build-runs/{}/retry",
            asset.organization_id, failed.id
        ),
        "postgres-retry-hosted-build",
        failed.id.to_string().as_bytes(),
    )?;
    let retry_request = RequestBuildRetryBundle {
        retry: retry.clone(),
        expected_previous_version: failed.aggregate_version,
        idempotency: retry_idempotency.clone(),
    };
    let (left, right) = tokio::join!(
        left_repository.request_retry(retry_request.clone()),
        right_repository.request_retry(retry_request),
    );
    let retries = [left?, right?];
    assert_eq!(retries.iter().filter(|result| result.replayed).count(), 1);
    assert!(retries.iter().all(|result| result.value == retry));
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of_build_run_id, Some(first_attempt_id));
    assert_eq!(retry.subject, failed.subject);
    assert_eq!(retry.asset_id(), Some(asset.id));
    assert_eq!(retry.asset_release_id(), Some(release.id));
    assert_eq!(
        left_repository
            .find_by_asset_release(asset.organization_id, release.id)
            .await?,
        Some(retry.clone())
    );
    assert!(left_repository
        .find_by_asset_release(OrganizationId::new(), release.id)
        .await?
        .is_none());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from build_runs where organization_id = ",)
                    .bind(asset.organization_id.as_uuid())
                    .append(" and asset_release_id = ")
                    .bind(release.id.as_uuid()),
            )
            .await?,
        2
    );

    // The retry is picked up by the same reconciler and cloud.build Flow.
    let restarted = BuildRunReconciler::new(left_repository.clone(), operations.clone());
    let repaired = restarted.run_once(10).await?;
    assert_eq!(repaired.reserved, 0);
    assert_eq!(repaired.started, 1);
    assert_eq!(repaired.replayed, 0);
    assert!(repaired.failures.is_empty());
    let retry_operation = operations
        .find_request(retry.operation_id)
        .await?
        .ok_or("hosted retry operation was not enqueued")?;
    assert_eq!(retry_operation.organization_id, asset.organization_id);
    assert_eq!(retry_operation.workflow.name(), BUILD_WORKFLOW_NAME);
    assert_eq!(retry_operation.workflow.version(), BUILD_WORKFLOW_VERSION);
    assert_eq!(
        retry_operation.input["buildRunId"],
        serde_json::Value::String(retry.id.to_string())
    );

    let mut retry = retry;
    let queued_version = retry.aggregate_version;
    retry.begin_preparation(retry.updated_at + Duration::milliseconds(1))?;
    let retry = left_repository.save(retry, queued_version).await?;
    let published = drive_hosted_release_publication(executor, asset, &release, retry).await?;
    assert_eq!(published.state, AssetReleaseState::Published);
    assert!(published.artifact.is_some());
    assert!(published.provenance.is_some());
    assert_eq!(publication_events().await?, 1);

    Ok(())
}

pub async fn publish_hosted_release(
    executor: &PostgresExecutor,
    asset: &Asset,
    release: &AssetRelease,
) -> Result<AssetRelease, Box<dyn std::error::Error>> {
    let mut build = BuildRun::reserve_asset_release(
        asset.organization_id,
        asset.id,
        release.id,
        chrono::Utc::now().max(release.updated_at),
    );
    let database = Database::new(PostgresDialect, executor.clone());
    let inserted = database
        .execute(
            sql_query::<()>(
                "insert into build_runs (organization_id, subject_kind, project_id, environment_id, source_revision_id, asset_id, asset_release_id, id, attempt, retry_of_build_run_id, operation_id, status, evidence_required, aggregate_version, requested_at, updated_at) values (",
            )
            .bind(build.organization_id.as_uuid())
            .append(", 'asset_release', null, null, null, ")
            .bind(asset.id.as_uuid())
            .append(", ")
            .bind(release.id.as_uuid())
            .append(", ")
            .bind(build.id.as_uuid())
            .append(", ")
            .bind(build.attempt)
            .append(", null, ")
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
    assert_eq!(inserted.rows_affected, 1);
    let operation = OperationRequest::new(
        build.operation_id,
        build.organization_id,
        OperationSubject::new("build_run", build.id.as_uuid())?,
        WorkflowIdentity::new(BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION)?,
        serde_json::json!({
            "organizationId": build.organization_id,
            "buildRunId": build.id,
        }),
        build.requested_at,
    );
    PostgresOperationRepository::new(executor.clone())
        .enqueue(operation)
        .await?;
    let expected = build.aggregate_version;
    build.begin_preparation(build.updated_at + Duration::milliseconds(1))?;
    let builds = PostgresBuildRunRepository::new(executor.clone());
    build = builds.save(build, expected).await?;
    drive_hosted_release_publication(executor, asset, release, build).await
}

async fn drive_hosted_release_publication(
    executor: &PostgresExecutor,
    asset: &Asset,
    release: &AssetRelease,
    mut build: BuildRun,
) -> Result<AssetRelease, Box<dyn std::error::Error>> {
    if build.status != BuildRunStatus::Preparing
        || build.organization_id != asset.organization_id
        || build.asset_id() != Some(asset.id)
        || build.asset_release_id() != Some(release.id)
    {
        return Err("hosted publication fixture received the wrong BuildRun".into());
    }
    let builds = PostgresBuildRunRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let node_id = NodeId::new();
    let apply_command_id = NodeCommandId::new();
    let cleanup_command_id = NodeCommandId::new();
    let command_time = build.updated_at;
    database
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, aggregate_version) values (",
            )
            .bind(asset.organization_id.as_uuid())
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", ")
            .bind(format!("hosted build {}", build.id))
            .append(", ")
            .bind(format!("hosted-build-{}", build.id))
            .append(", 'ready', ")
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
        (apply_command_id, 1_i64, "box_build_start"),
        (cleanup_command_id, 2_i64, "box_build_remove"),
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
                .bind(build.id.as_uuid())
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
                .bind(build.id.as_uuid())
                .append(")"),
            )
            .await?;
    }

    let mut at = build.updated_at;
    let input_artifact = build_artifact('a', 4_096)?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_input(
        format!("sha256:{}", "b".repeat(64)),
        input_artifact.clone(),
        at,
    )?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.schedule(node_id, format!("sha256:{}", "c".repeat(64)), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.dispatch(apply_command_id, at)?;
    build = builds.save(build, expected).await?;

    let output_artifact = build_artifact('d', 8_192)?;
    let box_output = box_output(&output_artifact, &input_artifact)?;
    let descriptor = OciDescriptor::new(
        box_output.descriptor.media_type.clone(),
        box_output.descriptor.digest.clone(),
        box_output.descriptor.size,
    )?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_validation(box_output, at)?;
    build = builds.save(build, expected).await?;

    let output = ValidatedOciBuildOutput {
        artifact: output_artifact,
        descriptor: descriptor.clone(),
        platforms: vec![BuildPlatform::parse("linux/amd64")?],
        content_bytes: 2_048,
        blob_count: 3,
    };
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_validated_output(output, at)?;
    build = builds.save(build, expected).await?;

    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s/assets/{}/releases/{}", asset.id, release.id),
        descriptor,
    )?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_publication(target.clone(), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_published_artifact(PublishedOciArtifact::from_target(&target), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_attestation(at)?;
    build = builds.save(build, expected).await?;

    let repository = format!(
        "https://a3s.dev/cloud/assets/{}/releases/{}",
        asset.id, release.id
    );
    let evidence = evidence_for(
        &build,
        at + Duration::milliseconds(1),
        &repository,
        release.commit_sha.as_str(),
        Some(release.manifest_digest.as_str()),
    )?;
    let provenance_digest = evidence.provenance_digest.clone();
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_evidence(evidence, at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_cleanup(cleanup_command_id, at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.complete(at)?;
    assert!(matches!(
        builds.save(build.clone(), expected).await,
        Err(RepositoryError::Conflict(_))
    ));
    let (left, right) = tokio::join!(
        builds.finalize(build.clone(), expected),
        builds.finalize(build, expected),
    );
    let finalized = [left?, right?];
    let BuildRunFinalization::Completed(succeeded) = &finalized[0] else {
        return Err("active hosted Asset publication was unexpectedly rejected".into());
    };
    let succeeded = succeeded.clone();
    assert_eq!(
        finalized[1],
        BuildRunFinalization::Completed(succeeded.clone())
    );
    assert_eq!(succeeded.status, BuildRunStatus::Succeeded);
    let replayed = builds
        .finalize(succeeded.clone(), succeeded.aggregate_version)
        .await?;
    assert_eq!(replayed, BuildRunFinalization::Completed(succeeded.clone()));

    let assets = PostgresAssetRepository::new(executor.clone());
    let published = assets
        .find_release(asset.organization_id, asset.id, release.id)
        .await?
        .ok_or("published hosted Asset release was not persisted")?;
    assert_eq!(published.state, AssetReleaseState::Published);
    let provenance = published
        .provenance
        .as_ref()
        .ok_or("published hosted Asset release omitted provenance")?;
    assert_eq!(provenance.build_run_id(), succeeded.id);
    assert_eq!(provenance.provenance_digest().as_str(), provenance_digest);
    Ok(published)
}

async fn attest_and_complete_published_build(
    builds: &Arc<PostgresBuildRunRepository>,
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    build_id: BuildRunId,
    published: BuildRun,
    cleanup_command_id: NodeCommandId,
) -> Result<PublishedOciArtifact, Box<dyn std::error::Error>> {
    let database = Database::new(PostgresDialect, executor.clone());
    let mut attesting = published.clone();
    attesting.begin_attestation(published.updated_at + Duration::milliseconds(1))?;
    let attesting = builds.save(attesting, published.aggregate_version).await?;
    assert_eq!(attesting.status, BuildRunStatus::Attesting);
    assert!(attesting.evidence.is_none());
    assert_eq!(builds.find(organization_id, build_id).await?, attesting);

    let mut attested = attesting.clone();
    let attested_at = attesting.updated_at + Duration::milliseconds(1);
    let evidence = evidence_for(
        &attesting,
        attested_at,
        "https://github.com/A3S-Lab/Cloud",
        &"a".repeat(40),
        None,
    )?;
    attested.record_evidence(evidence.clone(), attested_at)?;
    let attested = builds.save(attested, attesting.aggregate_version).await?;
    assert_eq!(attested.evidence.as_deref(), Some(&evidence));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<serde_json::Value>(
                    "select evidence from build_runs where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
            )
            .await?,
        serde_json::to_value(&evidence)?
    );

    let mut cleaning = attested.clone();
    cleaning.begin_cleanup(
        cleanup_command_id,
        attested.updated_at + Duration::milliseconds(1),
    )?;
    let cleaning = builds.save(cleaning, attested.aggregate_version).await?;
    let mut succeeded = cleaning.clone();
    succeeded.complete(cleaning.updated_at + Duration::milliseconds(1))?;
    assert!(matches!(
        builds
            .save(succeeded.clone(), cleaning.aggregate_version)
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let succeeded = builds
        .finalize(succeeded, cleaning.aggregate_version)
        .await?;
    let BuildRunFinalization::Completed(succeeded) = succeeded else {
        return Err("external BuildRun finalization was unexpectedly rejected".into());
    };
    assert_eq!(succeeded.status, BuildRunStatus::Succeeded);
    assert_eq!(succeeded.evidence.as_deref(), Some(&evidence));
    assert_eq!(builds.find(organization_id, build_id).await?, succeeded);
    assert!(
        database
            .execute(
                sql_query::<()>("update build_runs set evidence = null where organization_id = ",)
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(build_id.as_uuid()),
            )
            .await
            .is_err(),
        "PostgreSQL accepted a succeeded evidence-required build without evidence"
    );
    assert!(
        database
            .execute(
                sql_query::<()>(
                    "update build_runs set evidence = jsonb_set(evidence, '{verificationState}', '\"unverified\"'::jsonb) where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
            )
            .await
            .is_err(),
        "PostgreSQL accepted a non-verified build evidence document"
    );
    assert!(
        database
            .execute(
                sql_query::<()>(
                    "update build_runs set evidence = evidence #- '{signingKey,publicKey}' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
            )
            .await
            .is_err(),
        "PostgreSQL accepted build evidence without an Ed25519 public key"
    );
    assert_eq!(builds.find(organization_id, build_id).await?, succeeded);

    let mut tampered_evidence = evidence.clone();
    tampered_evidence.envelope.signatures[0].signature = STANDARD.encode([0_u8; 64]);
    database
        .execute(
            sql_query::<()>("update build_runs set evidence = ")
                .bind(serde_json::to_value(&tampered_evidence)?)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
        )
        .await?;
    assert!(
        builds.find(organization_id, build_id).await.is_err(),
        "repository restore accepted a tampered build evidence signature"
    );
    database
        .execute(
            sql_query::<()>("update build_runs set evidence = ")
                .bind(serde_json::to_value(&evidence)?)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(build_id.as_uuid()),
        )
        .await?;
    assert_eq!(builds.find(organization_id, build_id).await?, succeeded);
    succeeded
        .published_artifact
        .clone()
        .ok_or_else(|| "succeeded build omitted its published artifact".into())
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

fn box_output(
    output: &BuildArtifact,
    source: &BuildArtifact,
) -> Result<NodeBoxBuildOutput, Box<dyn std::error::Error>> {
    let digest = |fill: char| format!("sha256:{}", fill.to_string().repeat(64));
    let artifact = ArtifactRef {
        uri: output.uri.clone(),
        digest: output.digest.clone(),
        media_type: output.media_type.clone(),
    };
    let descriptor = NodeBoxBuildDescriptor {
        media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        digest: digest('e'),
        size: 512,
    };
    let platform = NodeBoxBuildPlatform {
        os: "linux".into(),
        architecture: "amd64".into(),
        variant: None,
    };
    let receipt = NodeBoxBuildOutput {
        artifact: RuntimeOutputArtifact {
            name: BOX_BUILD_OUTPUT_NAME.into(),
            artifact: artifact.clone(),
            size_bytes: output.size_bytes,
        },
        descriptor: descriptor.clone(),
        platforms: vec![platform.clone()],
        manifest_count: 1,
        content_bytes: 2_048,
        blob_count: 3,
        blob_inventory_digest: digest('6'),
        caches: vec![NodeBoxBuildCacheOutput {
            operation_id: "postgres-fixture-linux-amd64".into(),
            artifact: RuntimeOutputArtifact {
                name: "build-cache-postgres-fixture".into(),
                artifact,
                size_bytes: output.size_bytes,
            },
            receipt: NodeBoxBuildCacheReceipt {
                schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                key: digest('f'),
                source_digest: source.digest.clone(),
                plan_digest: digest('9'),
                descriptor,
                platform,
                content_bytes: 1_024,
                entry_count: 3,
                blob_count: 3,
                blob_inventory_digest: digest('7'),
            },
        }],
    };
    receipt.validate()?;
    Ok(receipt)
}
