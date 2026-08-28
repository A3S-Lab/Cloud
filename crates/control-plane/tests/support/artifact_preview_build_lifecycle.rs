use super::*;
use a3s_cloud_control_plane::modules::artifacts::{
    BuildCandidateProjector, BuildRunStatus, IArtifactBuildProjectionPort, IBuildRunRepository,
    PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::integration_events::{
    IIntegrationEventProjector, OutboxMessage,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PullRequestPreviewId, SourcePullRequestChangeId, SourceRevisionId, SourceSubscriptionId,
};
use a3s_cloud_control_plane::modules::sources::published::{
    PreviewSourceRevisionLifecycleState, PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
};
use chrono::Duration as ChronoDuration;

pub(super) async fn exercise_artifact_preview_build_lifecycle(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, String)>(
                    "select count(*), max(name) from a3s_orm_migrations where version = ",
                )
                .bind("162"),
            )
            .await?,
        (1, "Artifacts Preview build lifecycle projections".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let source_environment_id = EnvironmentId::new();
    let preview_environment_id = EnvironmentId::new();
    let source_subscription_id = SourceSubscriptionId::new();
    let preview_id = PullRequestPreviewId::new();
    let source_revision_id = SourceRevisionId::new();
    let correlation_id = Uuid::now_v7();
    let created_at = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("canonical test timestamp");
    super::developer_preview_policies_support::seed_scope_and_subscription(
        &database,
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        &[],
        created_at,
    )
    .await?;
    database
        .execute(
            sql_query::<()>("insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(preview_environment_id.as_uuid())
                .append(", 'Artifact Preview environment', 'artifact-preview', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let accepted_at = created_at + ChronoDuration::seconds(1);
    let commit_sha = "a".repeat(40);
    database
        .execute(
            sql_query::<()>("insert into external_source_revisions (organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at) select organization_id, project_id, ")
                .bind(preview_environment_id.as_uuid())
                .append(", ")
                .bind(source_revision_id.as_uuid())
                .append(", repository_provider, repository_url, repository_identity, ")
                .bind(commit_sha.clone())
                .append(", recipe, recipe_digest, 1, ")
                .bind(accepted_at)
                .append(" from github_repository_subscriptions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(source_subscription_id.as_uuid()),
        )
        .await?;
    let recipe_digest = database
        .fetch_one_as(
            sql_query::<String>(
                "select recipe_digest from external_source_revisions where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(source_revision_id.as_uuid()),
        )
        .await?;

    let fixture = LifecycleFixture {
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        preview_id,
        preview_environment_id,
        correlation_id,
        source_revision_id,
        commit_sha,
        recipe_digest,
        accepted_at,
    };
    let active = lifecycle_message(
        &fixture,
        1,
        PreviewSourceRevisionLifecycleState::Active,
        accepted_at + ChronoDuration::seconds(1),
    );
    persist_owner_fact(&database, &active).await?;

    let left_repository = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let right_repository = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let left_projection: Arc<dyn IArtifactBuildProjectionPort> = left_repository.clone();
    let right_projection: Arc<dyn IArtifactBuildProjectionPort> = right_repository.clone();
    let left_projector = BuildCandidateProjector::new(left_projection);
    let right_projector = BuildCandidateProjector::new(right_projection);
    let (left_active, right_active) = tokio::join!(
        left_projector.project(&active),
        right_projector.project(&active)
    );
    left_active?;
    right_active?;

    let (left_reserved, right_reserved) = tokio::join!(
        left_repository.reserve_pending(2),
        right_repository.reserve_pending(2)
    );
    let mut reserved = left_reserved?;
    reserved.extend(right_reserved?);
    assert_eq!(
        reserved.len(),
        1,
        "one Preview fact must admit one BuildRun"
    );
    let first = reserved.pop().expect("one Preview BuildRun");
    assert_eq!(first.status, BuildRunStatus::Queued);
    assert_eq!(first.source_revision_id(), Some(source_revision_id));
    assert_eq!(first.environment_id(), Some(preview_environment_id));
    assert_eq!(first.requested_at, accepted_at);
    assert_eq!(first.attempt, 1);

    let cleanup_at = active.occurred_at + ChronoDuration::seconds(2);
    let cleanup = lifecycle_message(
        &fixture,
        3,
        PreviewSourceRevisionLifecycleState::CleanupRequired,
        cleanup_at,
    );
    persist_owner_fact(&database, &cleanup).await?;
    let restarted_left = BuildCandidateProjector::new(left_repository.clone());
    let restarted_right = BuildCandidateProjector::new(right_repository.clone());
    let (left_cleanup, right_cleanup) = tokio::join!(
        restarted_left.project(&cleanup),
        restarted_right.project(&cleanup)
    );
    left_cleanup?;
    right_cleanup?;
    let cancelling = left_repository.find(organization_id, first.id).await?;
    assert_eq!(cancelling.status, BuildRunStatus::Cancelling);
    assert!(cancelling.cancellation_requested_at.is_some());
    assert!(left_repository.reserve_pending(1).await?.is_empty());

    let late_active = lifecycle_message(
        &fixture,
        2,
        PreviewSourceRevisionLifecycleState::Active,
        active.occurred_at + ChronoDuration::seconds(1),
    );
    persist_owner_fact(&database, &late_active).await?;
    BuildCandidateProjector::new(left_repository.clone())
        .project(&late_active)
        .await?;
    assert_eq!(
        left_repository.find(organization_id, first.id).await?,
        cancelling,
        "late active fact must not reopen a retired Preview build"
    );

    let mut cancelled = cancelling.clone();
    cancelled.complete(cleanup_at + ChronoDuration::seconds(1))?;
    let cancelled = left_repository
        .finalize(cancelled, cancelling.aggregate_version)
        .await?;
    assert_eq!(cancelled.status, BuildRunStatus::Cancelled);

    let reopened = lifecycle_message(
        &fixture,
        4,
        PreviewSourceRevisionLifecycleState::Active,
        cleanup_at + ChronoDuration::seconds(2),
    );
    persist_owner_fact(&database, &reopened).await?;
    BuildCandidateProjector::new(right_repository.clone())
        .project(&reopened)
        .await?;
    let (left_retry, right_retry) = tokio::join!(
        left_repository.reserve_pending(2),
        right_repository.reserve_pending(2)
    );
    let mut retries = left_retry?;
    retries.extend(right_retry?);
    assert_eq!(
        retries.len(),
        1,
        "one exact retirement receipt must authorize one retry"
    );
    let retry = retries.pop().expect("one Preview BuildRun retry");
    assert_eq!(retry.status, BuildRunStatus::Queued);
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of_build_run_id, Some(first.id));
    assert_eq!(retry.requested_at, reopened.occurred_at);
    assert!(left_repository.reserve_pending(1).await?.is_empty());

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, Option<Uuid>)>(
                    "select count(*), (array_agg(preview_id order by preview_id::text))[1] from artifact_build_candidates where organization_id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await?,
        (1, Some(preview_id.as_uuid()))
    );
    assert_eq!(
        database
            .fetch_all_as(
                sql_query::<(i64, String, String, Option<Uuid>, Option<Uuid>)>(
                    "select preview_aggregate_version, outcome, retirement, retired_source_revision_id, retired_build_run_id from artifact_preview_build_lifecycle_projections where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and preview_id = ")
                .bind(preview_id.as_uuid())
                .append(" order by preview_aggregate_version"),
            )
            .await?
            .rows,
        vec![
            (1, "applied".into(), "not_required".into(), None, None),
            (2, "ignored_stale".into(), "not_required".into(), None, None),
            (
                3,
                "applied".into(),
                "cancellation_requested".into(),
                Some(source_revision_id.as_uuid()),
                Some(first.id.as_uuid()),
            ),
            (4, "applied".into(), "not_required".into(), None, None),
        ]
    );

    let candidate_mutation = database
        .execute(
            sql_query::<()>("update artifact_build_candidates set preview_id = preview_id where organization_id = ")
                .bind(organization_id.as_uuid()),
        )
        .await
        .expect_err("build candidate mutation must fail");
    assert_eq!(
        super::developer_preview_policies_support::database_error_message(&candidate_mutation),
        Some("Artifact build candidate fact projections are immutable")
    );
    let receipt_mutation = database
        .execute(
            sql_query::<()>("update artifact_preview_build_lifecycle_projections set outcome = outcome where organization_id = ")
                .bind(organization_id.as_uuid()),
        )
        .await
        .expect_err("Preview build receipt mutation must fail");
    assert_eq!(
        super::developer_preview_policies_support::database_error_message(&receipt_mutation),
        Some("Preview build lifecycle projection receipts are immutable")
    );
    Ok(())
}

struct LifecycleFixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    preview_id: PullRequestPreviewId,
    preview_environment_id: EnvironmentId,
    correlation_id: Uuid,
    source_revision_id: SourceRevisionId,
    commit_sha: String,
    recipe_digest: String,
    accepted_at: chrono::DateTime<Utc>,
}

fn lifecycle_message(
    fixture: &LifecycleFixture,
    version: u64,
    state: PreviewSourceRevisionLifecycleState,
    occurred_at: chrono::DateTime<Utc>,
) -> OutboxMessage {
    let active = state == PreviewSourceRevisionLifecycleState::Active;
    OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY.into(),
        schema_version: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
        scope: a3s_cloud_control_plane::modules::shared_kernel::domain::ScopeContext::organization(
            a3s_cloud_control_plane::modules::shared_kernel::domain::InstallationId::new(),
            a3s_cloud_control_plane::modules::shared_kernel::domain::OrganizationId::from_uuid(
                fixture.organization_id.as_uuid(),
            ),
        )
        .expect("scope"),
        aggregate_id: fixture.preview_id.as_uuid(),
        aggregate_version: version,
        occurred_at,
        correlation_id: fixture.correlation_id,
        causation_id: Some(Uuid::now_v7()),
        payload: json!({
            "source_pull_request_change_id": SourcePullRequestChangeId::new(),
            "organization_id": fixture.organization_id,
            "project_id": fixture.project_id,
            "source_environment_id": fixture.source_environment_id,
            "source_subscription_id": fixture.source_subscription_id,
            "preview_id": fixture.preview_id,
            "preview_aggregate_version": version,
            "preview_environment_id": fixture.preview_environment_id,
            "state": state.as_str(),
            "source_revision_id": active.then_some(fixture.source_revision_id),
            "repository_identity": active.then_some("github:github.com/a3s-lab/cloud"),
            "commit_sha": active.then_some(fixture.commit_sha.as_str()),
            "recipe_digest": active.then_some(fixture.recipe_digest.as_str()),
            "source_revision_accepted_at": active.then_some(fixture.accepted_at),
        }),
        delivery_attempts: 1,
    }
}

async fn persist_owner_fact(
    database: &Database<PostgresDialect, PostgresExecutor>,
    message: &OutboxMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload, delivery_attempts) values (")
                .bind(message.event_id)
                .append(", ")
                .bind(message.event_key.clone())
                .append(", ")
                .bind(message.schema_version)
                .append(", ")
                .bind(message.organization_id())
                .append(", ")
                .bind(message.aggregate_id)
                .append(", ")
                .bind(message.aggregate_version)
                .append(", ")
                .bind(message.occurred_at)
                .append(", ")
                .bind(message.correlation_id)
                .append(", ")
                .bind(message.causation_id)
                .append(", ")
                .bind(message.payload.clone())
                .append(", 1)"),
        )
        .await?;
    Ok(())
}
