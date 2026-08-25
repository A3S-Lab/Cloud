use super::*;
use a3s_cloud_control_plane::modules::developer_workflows::{
    AcceptedPullRequestPreviewPolicyRevision, IPullRequestPreviewPolicyRepository,
    IPullRequestPreviewProjectionPort, IPullRequestPreviewProjectionRepository,
    PostgresPullRequestPreviewPolicyRepository, PostgresPullRequestPreviewProjectionRepository,
    PullRequestPreviewProjectionService, PullRequestPreviewProjector,
};
use a3s_cloud_control_plane::modules::integration_events::{
    IIntegrationEventProjector, OutboxMessage,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PrincipalId, RepositoryError, SourcePullRequestChangeId, SourceSubscriptionId,
};
use a3s_cloud_control_plane::modules::sources::published::{
    GitProvider, GitRepository, PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
};
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
    let projector = PullRequestPreviewProjector::new(service);

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
                sql_query::<(i64, i64, i64, i64, i64)>(
                    "select (select count(*) from external_source_revisions where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from build_runs where organization_id = ")
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
        (0, 0, 0, 0, 0),
        "Preview projection must not create resource-owner state"
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
    Ok(())
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
        organization_id: organization_id.as_uuid(),
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
