use super::*;
use a3s_cloud_control_plane::modules::developer_workflows::{
    AcceptedPullRequestPreviewPolicyRevision, IPreviewEnvironmentPort,
    IPullRequestPreviewPolicyRepository, IPullRequestPreviewProjectionPort,
    IPullRequestPreviewProjectionRepository, PostgresPullRequestPreviewPolicyRepository,
    PostgresPullRequestPreviewProjectionRepository, ProjectsPreviewEnvironmentAdapter,
    PullRequestPreviewProjectionService, PullRequestPreviewProjector,
    PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
};
use a3s_cloud_control_plane::modules::integration_events::{
    IIntegrationEventProjector, OutboxMessage,
};
use a3s_cloud_control_plane::modules::projects::{
    domain::repositories::IEnvironmentRepository, PostgresProjectsRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PrincipalId, RepositoryError, SourcePullRequestChangeId, SourceSubscriptionId,
};
use a3s_cloud_control_plane::modules::sources::published::{
    GitProvider, GitRepository, PreviewSourceRevisionLifecycleCommittedFact,
    PreviewSourceRevisionLifecycleState, PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY, PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
    SOURCE_REVISION_ACCEPTED_EVENT_KEY,
};
use a3s_cloud_control_plane::modules::sources::{
    IPreviewSourceRevisionProjectionPort, PostgresSourceRevisionRepository,
    PullRequestPreviewSourceProjector,
};
use a3s_orm::{DecodeError, FromRow, FromValue, Row};
use chrono::Duration as ChronoDuration;

pub(super) async fn exercise_developer_pull_request_preview_projection(
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
                .bind("157"),
            )
            .await?,
        (
            1,
            "Developer Workflows pull-request Preview projections".into()
        )
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, String)>(
                    "select count(*), max(name) from a3s_orm_migrations where version = ",
                )
                .bind("159"),
            )
            .await?,
        (
            1,
            "Sources pull-request Preview SourceRevision projections".into()
        )
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let source_environment_id = EnvironmentId::new();
    let source_subscription_id = SourceSubscriptionId::new();
    let actor = PrincipalId::new();
    let created_at = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("canonical test timestamp");
    super::developer_preview_policies_support::seed_scope_and_subscription(
        &database,
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        &[actor],
        created_at,
    )
    .await?;

    let policies = Arc::new(PostgresPullRequestPreviewPolicyRepository::new(
        executor.clone(),
    ));
    let contract = super::developer_preview_policies_support::policy_contract(
        organization_id,
        project_id,
        source_subscription_id,
        actor,
        86_400,
    )?;
    let revision = AcceptedPullRequestPreviewPolicyRevision::accept(
        source_environment_id,
        contract,
        1,
        actor,
        created_at + ChronoDuration::seconds(1),
    )?;
    policies
        .accept(super::developer_preview_policies_support::policy_write(
            &revision,
            None,
            "accept-preview-projection-policy",
        )?)
        .await?;
    let later_contract = super::developer_preview_policies_support::policy_contract(
        organization_id,
        project_id,
        source_subscription_id,
        actor,
        172_800,
    )?;
    let later_revision = AcceptedPullRequestPreviewPolicyRevision::accept(
        source_environment_id,
        later_contract,
        2,
        actor,
        created_at + ChronoDuration::seconds(3),
    )?;
    policies
        .accept(super::developer_preview_policies_support::policy_write(
            &later_revision,
            Some(revision.id),
            "accept-later-preview-projection-policy",
        )?)
        .await?;

    let previews = Arc::new(PostgresPullRequestPreviewProjectionRepository::new(
        executor.clone(),
    ));
    let policy_port: Arc<dyn IPullRequestPreviewPolicyRepository> = policies;
    let preview_port: Arc<dyn IPullRequestPreviewProjectionRepository> = previews.clone();
    let service: Arc<dyn IPullRequestPreviewProjectionPort> = Arc::new(
        PullRequestPreviewProjectionService::new(policy_port, preview_port),
    );
    let environments: Arc<dyn IPreviewEnvironmentPort> =
        Arc::new(ProjectsPreviewEnvironmentAdapter::new(Arc::new(
            PostgresProjectsRepository::new(executor.clone()),
        )));
    let projector = PullRequestPreviewProjector::new(service, environments);
    let source_projection_port: Arc<dyn IPreviewSourceRevisionProjectionPort> =
        Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let source_projector = PullRequestPreviewSourceProjector::new(source_projection_port);

    let pull_request_id = 1_000_042;
    let opened = message(
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        SourcePullRequestChangeId::new(),
        pull_request_id,
        "opened",
        'a',
        created_at,
        created_at + ChronoDuration::seconds(2),
    );
    projector.project(&opened).await?;
    projector.project(&opened).await?;

    let mut drifted = opened.clone();
    drifted.event_id = Uuid::now_v7();
    drifted.payload["head_commit_sha"] = json!("f".repeat(40));
    assert!(matches!(
        projector.project(&drifted).await,
        Err(RepositoryError::Conflict(message))
            if message.contains("changed content or owner binding")
    ));

    let synchronized = message(
        organization_id,
        project_id,
        source_environment_id,
        source_subscription_id,
        SourcePullRequestChangeId::new(),
        pull_request_id,
        "synchronized",
        'b',
        created_at,
        created_at + ChronoDuration::seconds(3),
    );
    projector.project(&synchronized).await?;

    let lifecycle_messages = load_lifecycle_messages(&database, organization_id).await?;
    assert_eq!(lifecycle_messages.len(), 2);
    assert_eq!(lifecycle_messages[0].causation_id, Some(opened.event_id));
    assert_eq!(lifecycle_messages[0].correlation_id, opened.correlation_id);
    assert_eq!(lifecycle_messages[0].occurred_at, opened.occurred_at);
    assert_eq!(
        lifecycle_messages[1].causation_id,
        Some(synchronized.event_id)
    );
    assert_eq!(
        lifecycle_messages[1].correlation_id,
        synchronized.correlation_id
    );
    assert_eq!(lifecycle_messages[1].occurred_at, synchronized.occurred_at);
    for message in &lifecycle_messages {
        projector.project(message).await?;
    }
    for message in lifecycle_messages.iter().rev() {
        source_projector.project(message).await?;
    }

    let restarted_policies: Arc<dyn IPullRequestPreviewPolicyRepository> = Arc::new(
        PostgresPullRequestPreviewPolicyRepository::new(executor.clone()),
    );
    let restarted_previews: Arc<dyn IPullRequestPreviewProjectionRepository> = Arc::new(
        PostgresPullRequestPreviewProjectionRepository::new(executor.clone()),
    );
    let restarted_service: Arc<dyn IPullRequestPreviewProjectionPort> = Arc::new(
        PullRequestPreviewProjectionService::new(restarted_policies, restarted_previews),
    );
    let restarted_environments: Arc<dyn IPreviewEnvironmentPort> =
        Arc::new(ProjectsPreviewEnvironmentAdapter::new(Arc::new(
            PostgresProjectsRepository::new(executor.clone()),
        )));
    let restarted_projector =
        PullRequestPreviewProjector::new(restarted_service, restarted_environments);
    let restarted_source_projection_port: Arc<dyn IPreviewSourceRevisionProjectionPort> =
        Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let restarted_source_projector =
        PullRequestPreviewSourceProjector::new(restarted_source_projection_port);
    for message in &lifecycle_messages {
        restarted_projector.project(message).await?;
        restarted_source_projector.project(message).await?;
    }

    let restarted = PostgresPullRequestPreviewProjectionRepository::new(executor.clone());
    let preview = restarted
        .find_preview(
            organization_id,
            project_id,
            source_environment_id,
            source_subscription_id,
            pull_request_id,
        )
        .await?
        .expect("persisted Preview");
    assert_eq!(preview.aggregate_version, 2);
    assert_eq!(preview.policy_authority.revision_id, revision.id);
    assert_ne!(preview.policy_authority.revision_id, later_revision.id);
    assert_eq!(preview.head_commit_sha.as_str(), "b".repeat(40));
    let environment = PostgresProjectsRepository::new(executor.clone())
        .find(organization_id, project_id, preview.environment_id)
        .await?
        .expect("Projects Environment");
    assert_eq!(environment.aggregate_version, 1);
    assert_eq!(environment.created_at, preview.provider_created_at);
    assert_eq!(environment.name.as_str(), preview.environment_name());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from environments where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and id = ")
                    .bind(preview.environment_id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and event_key = 'project.environment.created' and aggregate_id = ")
                    .bind(preview.environment_id.as_uuid()),
            )
            .await?,
        1,
        "Environment handoff replay must not publish another Projects event"
    );
    assert_eq!(
        restarted
            .find_receipt(
                organization_id,
                SourcePullRequestChangeId::from_uuid(synchronized.aggregate_id),
            )
            .await?
            .expect("persisted receipt")
            .preview_aggregate_version,
        Some(2)
    );

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64)>(
                    "select (select count(*) from developer_pull_request_previews where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from developer_pull_request_change_projections where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(")"),
            )
            .await?,
        (1, 2)
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64, i64, i64, i64, i64)>(
                    "select (select count(*) from external_source_revisions where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from build_runs where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from artifact_build_candidates where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from workloads where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from routes where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from operation_requests where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(")"),
            )
            .await?,
        (1, 0, 0, 0, 0, 0),
        "Sources handoff must create only the latest ordinary SourceRevision, not bypass Artifacts or later owners"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<String>(
                    "select commit_sha from external_source_revisions where organization_id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await?,
        "b".repeat(40),
        "active Preview version 2 must fence late version 1"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64, i64)>(
                    "select (select count(*) from source_pull_request_preview_revision_projections where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = ")
                .bind(PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY)
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = ")
                .bind(SOURCE_REVISION_ACCEPTED_EVENT_KEY)
                .append(")"),
            )
            .await?,
        (2, 1, 0),
        "Sources must retain both immutable version receipts, publish only the latest specialized fact, and not bypass the Artifacts fence"
    );
    let source_lifecycle_messages = load_outbox_messages(
        &database,
        organization_id,
        PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    )
    .await?;
    assert_eq!(source_lifecycle_messages.len(), 1);
    let source_lifecycle = &source_lifecycle_messages[0];
    let source_fact: PreviewSourceRevisionLifecycleCommittedFact =
        serde_json::from_value(source_lifecycle.payload.clone())?;
    source_fact.validate()?;
    assert_eq!(
        source_fact.state(),
        PreviewSourceRevisionLifecycleState::Active
    );
    assert_eq!(source_fact.preview_aggregate_version(), 2);
    assert_eq!(source_lifecycle.aggregate_version, 2);
    assert_eq!(
        source_lifecycle.causation_id,
        Some(lifecycle_messages[1].event_id)
    );
    assert_eq!(
        source_lifecycle.correlation_id,
        lifecycle_messages[1].correlation_id
    );
    assert_eq!(
        source_fact.source_revision_accepted_at(),
        Some(lifecycle_messages[1].occurred_at),
        "the specialized fact must retain the ordinary SourceRevision creation time"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(String, String)>(
                    "select min(outcome), max(outcome) from source_pull_request_preview_revision_projections where organization_id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await?,
        ("ignored_stale".into(), "projected".into())
    );

    let preview_mutation = database
        .execute(
            sql_query::<()>("update developer_pull_request_previews set aggregate_version = aggregate_version where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(preview.id.as_uuid()),
        )
        .await
        .expect_err("Preview CAS bypass must fail");
    assert_eq!(
        super::developer_preview_policies_support::database_error_message(&preview_mutation),
        Some("pull-request Preview mutation changed immutable authority or skipped CAS")
    );
    let receipt_mutation = database
        .execute(
            sql_query::<()>("update developer_pull_request_change_projections set fact_digest = ")
                .bind(format!("sha256:{}", "f".repeat(64)))
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and source_pull_request_change_id = ")
                .bind(opened.aggregate_id),
        )
        .await
        .expect_err("projection receipt mutation must fail");
    assert_eq!(
        super::developer_preview_policies_support::database_error_message(&receipt_mutation),
        Some("pull-request change projection receipts are immutable")
    );
    let source_receipt_mutation = database
        .execute(
            sql_query::<()>(
                "update source_pull_request_preview_revision_projections set fact_digest = ",
            )
            .bind(format!("sha256:{}", "e".repeat(64)))
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and preview_id = ")
            .bind(preview.id.as_uuid())
            .append(" and preview_aggregate_version = 2"),
        )
        .await
        .expect_err("Sources Preview projection receipt mutation must fail");
    assert_eq!(
        super::developer_preview_policies_support::database_error_message(&source_receipt_mutation),
        Some("Preview Source revision projection receipts are immutable")
    );
    Ok(())
}

async fn load_lifecycle_messages(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
) -> Result<Vec<OutboxMessage>, Box<dyn std::error::Error>> {
    load_outbox_messages(
        database,
        organization_id,
        PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
    )
    .await
}

async fn load_outbox_messages(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    event_key: &str,
) -> Result<Vec<OutboxMessage>, Box<dyn std::error::Error>> {
    let rows = database
        .fetch_all_as(
            sql_query::<LifecycleOutboxRow>("select event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = ")
                .bind(event_key)
                .append(" order by aggregate_version"),
        )
        .await?;
    Ok(rows
        .rows
        .into_iter()
        .map(|row| OutboxMessage {
            event_id: row.event_id,
            event_key: row.event_key,
            schema_version: row.schema_version,
            scope: a3s_cloud_control_plane::modules::shared_kernel::domain::ScopeContext::organization(
                a3s_cloud_control_plane::modules::shared_kernel::domain::InstallationId::new(),
                a3s_cloud_control_plane::modules::shared_kernel::domain::OrganizationId::from_uuid(row.organization_id),
            ).expect("scope"),
            aggregate_id: row.aggregate_id,
            aggregate_version: row.aggregate_version,
            occurred_at: row.occurred_at,
            correlation_id: row.correlation_id,
            causation_id: row.causation_id,
            payload: row.payload,
            delivery_attempts: 1,
        })
        .collect())
}

struct LifecycleOutboxRow {
    event_id: Uuid,
    event_key: String,
    schema_version: u32,
    organization_id: Uuid,
    aggregate_id: Uuid,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
    payload: Value,
}

impl FromRow for LifecycleOutboxRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            event_id: decode(row, 0)?,
            event_key: decode(row, 1)?,
            schema_version: decode(row, 2)?,
            organization_id: decode(row, 3)?,
            aggregate_id: decode(row, 4)?,
            aggregate_version: decode(row, 5)?,
            occurred_at: decode(row, 6)?,
            correlation_id: decode(row, 7)?,
            causation_id: decode(row, 8)?,
            payload: decode(row, 9)?,
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

#[allow(clippy::too_many_arguments)]
fn message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    source_pull_request_change_id: SourcePullRequestChangeId,
    pull_request_id: u64,
    kind: &str,
    sha: char,
    provider_created_at: chrono::DateTime<Utc>,
    occurred_at: chrono::DateTime<Utc>,
) -> OutboxMessage {
    let repository = GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
        .expect("repository");
    OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY.into(),
        schema_version: PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
        scope: a3s_cloud_control_plane::modules::shared_kernel::domain::ScopeContext::organization(
            a3s_cloud_control_plane::modules::shared_kernel::domain::InstallationId::new(),
            a3s_cloud_control_plane::modules::shared_kernel::domain::OrganizationId::from_uuid(
                organization_id.as_uuid(),
            ),
        )
        .expect("scope"),
        aggregate_id: source_pull_request_change_id.as_uuid(),
        aggregate_version: 1,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: json!({
            "source_pull_request_change_id": source_pull_request_change_id,
            "organization_id": organization_id,
            "project_id": project_id,
            "environment_id": source_environment_id,
            "source_subscription_id": source_subscription_id,
            "installation_id": 42,
            "base_repository": repository,
            "base_branch": "main",
            "head_repository": repository,
            "head_branch": "feature/preview",
            "head_commit_sha": sha.to_string().repeat(40),
            "pull_request_id": pull_request_id,
            "pull_request_number": 42,
            "kind": kind,
            "merged": false,
            "provider_created_at": provider_created_at,
            "provider_updated_at": occurred_at - ChronoDuration::milliseconds(100),
        }),
        delivery_attempts: 1,
    }
}
