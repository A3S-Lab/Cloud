use super::*;
use a3s_cloud_control_plane::modules::developer_workflows::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    GitBranch, GithubInstallationRef, IPullRequestPreviewPolicyRepository,
    PostgresPullRequestPreviewPolicyRepository, PreviewForkPolicy, PreviewQuota,
    PullRequestPreviewPolicy, PullRequestPreviewPolicyContract,
    PullRequestPreviewPolicyRevisionAccepted,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{PrincipalId, SourceSubscriptionId};
use a3s_cloud_control_plane::modules::sources::published::{GitProvider, GitRepository};
use a3s_orm::DatabaseError;
use chrono::Duration as ChronoDuration;

pub(super) async fn exercise_developer_preview_policy_persistence(
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
                .bind("153"),
            )
            .await?,
        (1, "immutable pull-request Preview policy revisions".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let subscription_id = SourceSubscriptionId::new();
    let first_actor = PrincipalId::new();
    let second_actor = PrincipalId::new();
    let created_at = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("canonical test timestamp");
    seed_scope_and_subscription(
        &database,
        organization_id,
        project_id,
        environment_id,
        subscription_id,
        &[first_actor, second_actor],
        created_at,
    )
    .await?;

    let policies = PostgresPullRequestPreviewPolicyRepository::new(executor.clone());
    let initial_contract = policy_contract(
        organization_id,
        project_id,
        subscription_id,
        first_actor,
        86_400,
    )?;
    let first_revision = AcceptedPullRequestPreviewPolicyRevision::accept(
        environment_id,
        initial_contract.clone(),
        1,
        first_actor,
        created_at + ChronoDuration::seconds(1),
    )?;
    let first_write = policy_write(&first_revision, None, "accept-preview-policy-1")?;
    let first_idempotency = first_write.idempotency.clone();
    assert!(!policies.accept(first_write).await?.replayed);
    assert_eq!(
        policies.replay_acceptance(&first_idempotency).await?,
        Some(first_revision.clone())
    );

    let same_contract_candidate = AcceptedPullRequestPreviewPolicyRevision::accept(
        environment_id,
        initial_contract,
        2,
        second_actor,
        first_revision.accepted_at + ChronoDuration::seconds(1),
    )?;
    let adopted = policies
        .accept(policy_write(
            &same_contract_candidate,
            Some(first_revision.id),
            "adopt-preview-policy",
        )?)
        .await?;
    assert!(adopted.replayed);
    assert_eq!(adopted.value, first_revision);

    let changed_contract = policy_contract(
        organization_id,
        project_id,
        subscription_id,
        first_actor,
        172_800,
    )?;
    let second_revision = AcceptedPullRequestPreviewPolicyRevision::accept(
        environment_id,
        changed_contract,
        2,
        second_actor,
        first_revision.accepted_at + ChronoDuration::seconds(2),
    )?;
    let second_write = policy_write(
        &second_revision,
        Some(first_revision.id),
        "accept-preview-policy-2",
    )?;
    let second_idempotency = second_write.idempotency.clone();
    assert!(!policies.accept(second_write).await?.replayed);

    let restarted = PostgresPullRequestPreviewPolicyRepository::new(executor.clone());
    assert_eq!(
        restarted.replay_acceptance(&second_idempotency).await?,
        Some(second_revision.clone())
    );
    assert_eq!(
        restarted
            .find_revision(
                organization_id,
                project_id,
                environment_id,
                subscription_id,
                first_revision.id,
            )
            .await?,
        Some(first_revision.clone())
    );
    assert_eq!(
        restarted
            .find_current(organization_id, project_id, environment_id, subscription_id,)
            .await?,
        Some(second_revision.clone())
    );
    assert_eq!(
        restarted
            .list_revisions(
                organization_id,
                project_id,
                environment_id,
                subscription_id,
                10,
            )
            .await?,
        vec![first_revision.clone(), second_revision.clone()]
    );

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64)>(
                    "select (select count(*) from outbox_events where event_key = ",
                )
                .bind("developer.pull-request-preview-policy.revision-accepted")
                .append(" and organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(subscription_id.as_uuid())
                .append("), (select count(*) from audit_records where action = ")
                .bind("developer.pull-request-preview-policy.revision-accepted")
                .append(" and organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(subscription_id.as_uuid())
                .append(")"),
            )
            .await?,
        (2, 2)
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
                .append("), (select count(*) from operations where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(")"),
            )
            .await?,
        (0, 0, 0, 0, 0),
        "Preview policy acceptance must not create owner runtime resources"
    );

    let mutation = database
        .execute(
            sql_query::<()>("update developer_pull_request_preview_policy_revisions set accepted_at = accepted_at + interval '1 second' where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(second_revision.id.as_uuid()),
        )
        .await
        .expect_err("accepted Preview policy mutation must fail");
    assert_eq!(
        database_error_message(&mutation),
        Some("accepted Preview policy revisions are immutable")
    );

    let outsider = PrincipalId::new();
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at) values (")
                .bind(outsider.as_uuid())
                .append(", 'human', 'Preview policy outsider', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    let cross_tenant_owner = database
        .execute(
            clone_revision_insert()
                .bind(Uuid::now_v7())
                .append(", revision_number + 1, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, ")
                .bind(outsider.as_uuid())
                .append(", lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, accepted_by, accepted_at + interval '1 second' from developer_pull_request_preview_policy_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(second_revision.id.as_uuid()),
        )
        .await
        .expect_err("an owner outside the policy Organization must fail");
    assert_eq!(
        database_error_constraint(&cross_tenant_owner),
        Some("developer_preview_policy_owner_membership_fk")
    );

    let cross_tenant_actor = database
        .execute(
            clone_revision_insert()
                .bind(Uuid::now_v7())
                .append(", revision_number + 1, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, owner_principal_id, lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, ")
                .bind(outsider.as_uuid())
                .append(", accepted_at + interval '1 second' from developer_pull_request_preview_policy_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(second_revision.id.as_uuid()),
        )
        .await
        .expect_err("an accepting actor outside the policy Organization must fail");
    assert_eq!(
        database_error_constraint(&cross_tenant_actor),
        Some("developer_preview_policy_actor_membership_fk")
    );

    let sequence_gap = database
        .execute(
            clone_revision_insert()
                .bind(Uuid::now_v7())
                .append(", revision_number + 2, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, owner_principal_id, lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, accepted_by, accepted_at + interval '1 second' from developer_pull_request_preview_policy_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(second_revision.id.as_uuid()),
        )
        .await
        .expect_err("Preview policy sequence gap must fail");
    assert_eq!(
        database_error_message(&sequence_gap),
        Some("Preview policy revision sequence is not monotonic")
    );

    Ok(())
}

fn policy_contract(
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_subscription_id: SourceSubscriptionId,
    owner_principal_id: PrincipalId,
    lifetime_seconds: u32,
) -> Result<PullRequestPreviewPolicyContract, String> {
    PullRequestPreviewPolicyContract::from_policy(PullRequestPreviewPolicy {
        organization_id,
        project_id,
        source_subscription_id,
        owner_principal_id,
        installation_id: GithubInstallationRef::parse(42)?,
        base_repository: GitRepository::parse(
            GitProvider::Github,
            "https://github.com/a3s-lab/cloud",
        )?,
        base_branch: GitBranch::parse("main")?,
        lifetime_seconds,
        maximum_active_previews: 8,
        fork_policy: PreviewForkPolicy::Isolated,
        allow_protected_secrets_for_trusted_sources: true,
        quota: PreviewQuota {
            maximum_workloads: 4,
            cpu_millis: 2_000,
            memory_bytes: 1024 * 1024 * 1024,
            ephemeral_storage_bytes: 1024 * 1024 * 1024,
        },
    })
}

fn policy_write(
    revision: &AcceptedPullRequestPreviewPolicyRevision,
    expected_previous_revision_id: Option<
        a3s_cloud_control_plane::modules::shared_kernel::domain::PullRequestPreviewPolicyRevisionId,
    >,
    key: &str,
) -> Result<AcceptPullRequestPreviewPolicyRevisionWrite, String> {
    let request_id = Uuid::now_v7();
    Ok(AcceptPullRequestPreviewPolicyRevisionWrite {
        revision: revision.clone(),
        expected_previous_revision_id,
        event: PullRequestPreviewPolicyRevisionAccepted::envelope(revision, request_id)?,
        actor_principal_id: revision.accepted_by,
        request_id,
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/pull-request-preview-policies",
                revision.organization_id, revision.project_id, revision.source_environment_id
            ),
            key,
            revision.contract.digest().as_str().as_bytes(),
        )?,
    })
}

#[allow(clippy::too_many_arguments)]
async fn seed_scope_and_subscription(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    subscription_id: SourceSubscriptionId,
    actors: &[PrincipalId],
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'Preview policy tenant', ")
                .bind(format!("preview-policy-{organization_id}"))
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", 'Preview policy project', 'preview-policy-project', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", 'Preview source environment', 'preview-source', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    for (index, actor) in actors.iter().enumerate() {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at) values (")
                    .bind(actor.as_uuid())
                    .append(", 'human', ")
                    .bind(format!("Preview policy actor {index}"))
                    .append(", 1, ")
                    .bind(created_at)
                    .append(")"),
            )
            .await?;
        database
            .execute(
                sql_query::<()>("insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (")
                    .bind(Uuid::now_v7())
                    .append(", ")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(actor.as_uuid())
                    .append(", 'owner', 1, ")
                    .bind(created_at)
                    .append(", ")
                    .bind(created_at)
                    .append(", null)"),
            )
            .await?;
    }
    let connection_id = Uuid::now_v7();
    database
        .execute(
            sql_query::<()>("insert into github_source_connections (organization_id, id, installation_id, account_id, account_login, account_kind, verified_by_user_id, verified_by_user_login, aggregate_version, connected_at, status, updated_at, provider_checked_at, provider_check_attempted_at, provider_next_check_at, provider_check_failures, provider_check_error) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(connection_id)
                .append(", 42, 1001, 'a3s-lab', 'organization', 1002, 'preview-operator', 1, ")
                .bind(created_at)
                .append(", 'active', ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", 0, null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into github_repository_subscriptions (organization_id, project_id, environment_id, id, connection_id, installation_id, repository_provider, repository_url, repository_identity, branch_name, recipe, recipe_digest, status, aggregate_version, created_at, deactivated_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", ")
                .bind(subscription_id.as_uuid())
                .append(", ")
                .bind(connection_id)
                .append(", 42, 'github', 'https://github.com/a3s-lab/cloud', 'github:github.com/a3s-lab/cloud', 'main', ")
                .bind(json!({
                    "schema": "a3s.cloud.build-recipe.v1",
                    "kind": "dockerfile",
                    "contextPath": ".",
                    "dockerfilePath": "Dockerfile",
                    "target": null,
                    "platforms": ["linux/amd64"]
                }))
                .append(", ")
                .bind(format!("sha256:{}", "a".repeat(64)))
                .append(", 'active', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    Ok(())
}

fn clone_revision_insert() -> a3s_orm::SqlQuery<()> {
    sql_query::<()>("insert into developer_pull_request_preview_policy_revisions (organization_id, project_id, source_environment_id, source_subscription_id, id, revision_number, installation_id, repository_provider, repository_url, repository_identity, base_branch, policy_schema, canonical_acl, policy_digest, owner_principal_id, lifetime_seconds, maximum_active_previews, fork_policy, allow_protected_secrets_for_trusted_sources, maximum_workloads, cpu_millis, memory_bytes, ephemeral_storage_bytes, accepted_by, accepted_at) select organization_id, project_id, source_environment_id, source_subscription_id, ")
}

fn database_error_message(error: &DatabaseError<PostgresError>) -> Option<&str> {
    let DatabaseError::Execute(PostgresError::Database(error)) = error else {
        return None;
    };
    error.as_db_error().map(|error| error.message())
}

fn database_error_constraint(error: &DatabaseError<PostgresError>) -> Option<&str> {
    let DatabaseError::Execute(PostgresError::Database(error)) = error else {
        return None;
    };
    error.as_db_error().and_then(|error| error.constraint())
}
