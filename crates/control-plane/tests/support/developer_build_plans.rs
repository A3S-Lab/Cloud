use super::*;
use a3s_cloud_control_plane::modules::developer_workflows::{
    AcceptBuildPlanWrite, AcceptedBuildPlan, AcceptedBuildPlanContract, BuildPlanAccepted,
    BuildPlanDetectorKind, BuildPlanProposal, BuildPlanProposalSpec, IBuildPlanRepository,
    PostgresBuildPlanRepository, SourceLayoutIdentity, BUILD_PLAN_DETECTOR_REVISION,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PrincipalId, Sha256Digest, SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::{
    domain::{
        AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider,
        GitRepository, ISourceRevisionRepository, NewExternalSourceRevision,
        SourceRevisionAccepted,
    },
    PostgresSourceRevisionRepository,
};
use a3s_orm::DatabaseError;
use chrono::Duration as ChronoDuration;

pub(super) async fn exercise_developer_build_plan_persistence(
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
                .bind("146"),
            )
            .await?,
        (1, "immutable accepted developer BuildPlans".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    seed_scope(
        &database,
        organization_id,
        project_id,
        environment_id,
        actor,
        created_at,
    )
    .await?;

    let repository = GitRepository::parse(
        GitProvider::Github,
        "https://github.com/a3s-lab/developer-build-plan-fixture",
    )?;
    let commit_sha = GitCommitSha::parse("1".repeat(40))?;
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )?;
    let source_revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: SourceRevisionId::new(),
        repository,
        commit_sha: commit_sha.clone(),
        recipe: recipe.clone(),
        accepted_at: created_at + ChronoDuration::seconds(1),
    })?;
    let source_request_id = Uuid::now_v7();
    let source_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"
        ),
        "developer-build-plan-source",
        source_revision.id.as_uuid().as_bytes(),
    )?;
    PostgresSourceRevisionRepository::new(executor.clone())
        .accept(AcceptSourceRevision {
            event: SourceRevisionAccepted::envelope(&source_revision, source_request_id)?,
            revision: source_revision.clone(),
            webhook_delivery: None,
            idempotency: source_idempotency,
        })
        .await?;

    let initial_proposal = proposal(&source_revision, recipe, digest('b'))?;
    let contract = AcceptedBuildPlanContract::from_proposal(source_revision.id, initial_proposal)?;
    let plan = AcceptedBuildPlan::accept(
        organization_id,
        project_id,
        environment_id,
        contract,
        actor,
        source_revision.accepted_at + ChronoDuration::seconds(1),
    )?;
    let plans = PostgresBuildPlanRepository::new(executor.clone());
    let first = write(&plan, actor, "accept-build-plan")?;
    let first_idempotency = first.idempotency.clone();
    assert!(!plans.accept(first.clone()).await?.replayed);
    assert_eq!(plans.accept(first).await?.value, plan);
    assert_eq!(
        plans.replay_acceptance(&first_idempotency).await?,
        Some(plan.clone())
    );

    let restarted = PostgresBuildPlanRepository::new(executor.clone());
    let adopted = restarted
        .accept(write(&plan, actor, "adopt-build-plan")?)
        .await?;
    assert!(adopted.replayed);
    assert_eq!(adopted.value, plan);
    assert_eq!(
        restarted
            .find(organization_id, project_id, environment_id, plan.id,)
            .await?,
        Some(plan.clone())
    );
    assert_eq!(
        restarted
            .list_for_source(
                organization_id,
                project_id,
                environment_id,
                source_revision.id,
                10,
            )
            .await?,
        vec![plan.clone()]
    );

    let changed = proposal(
        &source_revision,
        source_revision.recipe.clone(),
        digest('e'),
    )?;
    let competing = AcceptedBuildPlan::accept(
        organization_id,
        project_id,
        environment_id,
        AcceptedBuildPlanContract::from_proposal(source_revision.id, changed)?,
        actor,
        plan.accepted_at + ChronoDuration::seconds(1),
    )?;
    assert!(matches!(
        restarted
            .accept(write(&competing, actor, "competing-build-plan")?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where event_key = ",)
                    .bind("developer.build-plan.accepted")
                    .append(" and organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and aggregate_id = ")
                    .bind(plan.id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from audit_records where action = ")
                    .bind("developer.build-plan.accepted")
                    .append(" and organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and aggregate_id = ")
                    .bind(plan.id.as_uuid()),
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from build_runs where organization_id = ",)
                    .bind(organization_id.as_uuid())
                    .append(" and source_revision_id = ")
                    .bind(source_revision.id.as_uuid()),
            )
            .await?,
        0,
        "BuildPlan acceptance must not start a BuildRun"
    );

    let mutation = database
        .execute(
            sql_query::<()>("update developer_build_plans set accepted_at = accepted_at + interval '1 second' where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(plan.id.as_uuid()),
        )
        .await
        .expect_err("accepted BuildPlan mutation must fail");
    assert_eq!(
        database_error_message(&mutation),
        Some("accepted BuildPlans are immutable")
    );

    let mismatched_identity = database
        .execute(
            sql_query::<()>("insert into developer_build_plans (organization_id, project_id, environment_id, id, source_revision_id, project_root, contract_schema, canonical_acl, contract_digest, proposal_digest, source_identity_digest, commit_sha, source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at) select organization_id, project_id, environment_id, ")
                .bind(Uuid::now_v7())
                .append(", source_revision_id, 'other-identity', contract_schema, canonical_acl, contract_digest, proposal_digest, ")
                .bind(format!("sha256:{}", "d".repeat(64)))
                .append(", commit_sha, source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at from developer_build_plans where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(plan.id.as_uuid()),
        )
        .await
        .expect_err("mismatched Source identity must fail");
    assert_eq!(
        database_error_message(&mismatched_identity),
        Some("accepted BuildPlan does not match its exact Source revision")
    );

    let mismatched_commit = database
        .execute(
            sql_query::<()>("insert into developer_build_plans (organization_id, project_id, environment_id, id, source_revision_id, project_root, contract_schema, canonical_acl, contract_digest, proposal_digest, source_identity_digest, commit_sha, source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at) select organization_id, project_id, environment_id, ")
                .bind(Uuid::now_v7())
                .append(", source_revision_id, 'other-commit', contract_schema, canonical_acl, contract_digest, proposal_digest, source_identity_digest, ")
                .bind("2".repeat(40))
                .append(", source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at from developer_build_plans where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(plan.id.as_uuid()),
        )
        .await
        .expect_err("mismatched Source evidence must fail");
    assert_eq!(
        database_error_message(&mismatched_commit),
        Some("accepted BuildPlan does not match its exact Source revision")
    );

    Ok(())
}

fn proposal(
    source: &ExternalSourceRevision,
    recipe: BuildRecipe,
    content_digest: Sha256Digest,
) -> Result<BuildPlanProposal, String> {
    BuildPlanProposal::from_spec(BuildPlanProposalSpec {
        source: SourceLayoutIdentity::new(
            Sha256Digest::parse(source.source_identity_digest())?,
            source.commit_sha.clone(),
            content_digest,
        )?,
        detector: BuildPlanDetectorKind::Dockerfile,
        detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
        project_root: ".".into(),
        evidence_path: "Dockerfile".into(),
        evidence_digest: digest('c'),
        recipe,
    })
}

fn write(
    plan: &AcceptedBuildPlan,
    actor: PrincipalId,
    key: &str,
) -> Result<AcceptBuildPlanWrite, String> {
    let request_id = Uuid::now_v7();
    Ok(AcceptBuildPlanWrite {
        event: BuildPlanAccepted::envelope(plan, request_id)?,
        actor_principal_id: actor,
        request_id,
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/build-plans",
                plan.organization_id, plan.project_id, plan.environment_id
            ),
            key,
            plan.contract.canonical_acl().as_bytes(),
        )?,
        plan: plan.clone(),
    })
}

async fn seed_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'Developer workflows tenant', ")
                .bind(format!("developer-workflows-{}", organization_id.as_uuid()))
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
                .append(", 'Developer workflows project', 'developer-workflows-project', 1, ")
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
                .append(", 'Developer workflows environment', 'developer-workflows-environment', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Developer workflows actor', 1, ")
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
    Ok(())
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("test digest")
}

fn database_error_message(error: &DatabaseError<PostgresError>) -> Option<&str> {
    let DatabaseError::Execute(PostgresError::Database(error)) = error else {
        return None;
    };
    error.as_db_error().map(|error| error.message())
}
