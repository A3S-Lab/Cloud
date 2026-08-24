use super::*;
use a3s_cloud_control_plane::modules::developer_workflows::{
    AcceptBuildPlanWrite, AcceptWorkloadProfileRevisionWrite, AcceptedBuildPlan,
    AcceptedBuildPlanContract, AcceptedWorkloadProfileRevision, BuildPlanAccepted,
    BuildPlanDetectorKind, BuildPlanProposal, BuildPlanProposalSpec, IBuildPlanRepository,
    IWorkloadProfileRepository, PostgresBuildPlanRepository, PostgresWorkloadProfileRepository,
    SourceLayoutIdentity, WorkloadHttpHealthCheck, WorkloadProcess, WorkloadProfileContract,
    WorkloadProfileKind, WorkloadProfileResources, WorkloadProfileRevisionAccepted,
    WorkloadProfileSpec, WorkloadServicePort, BUILD_PLAN_DETECTOR_REVISION,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PrincipalId, Sha256Digest, SourceRevisionId, WorkloadProfileRevisionId,
};
use a3s_cloud_control_plane::modules::sources::{
    domain::{
        AcceptSourceRevision, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
        ISourceRevisionRepository, NewExternalSourceRevision, SourceRevisionAccepted,
    },
    published::BuildRecipe,
    PostgresSourceRevisionRepository,
};
use a3s_orm::DatabaseError;
use chrono::Duration as ChronoDuration;
use std::collections::BTreeMap;

pub(super) async fn exercise_developer_workload_profile_persistence(
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
                .bind("147"),
            )
            .await?,
        (
            1,
            "immutable accepted developer workload profile revisions".into()
        )
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let first_actor = PrincipalId::new();
    let second_actor = PrincipalId::new();
    let created_at = Utc::now();
    seed_scope(
        &database,
        organization_id,
        project_id,
        environment_id,
        &[first_actor, second_actor],
        created_at,
    )
    .await?;

    let source_revision = seed_source_revision(
        executor.clone(),
        organization_id,
        project_id,
        environment_id,
        created_at,
    )
    .await?;
    let build_plan = accepted_build_plan(
        &source_revision,
        organization_id,
        project_id,
        environment_id,
        first_actor,
    )?;
    PostgresBuildPlanRepository::new(executor.clone())
        .accept(build_plan_write(
            &build_plan,
            "workload-profile-build-plan",
        )?)
        .await?;

    let profiles = PostgresWorkloadProfileRepository::new(executor.clone());
    let initial_contract = WorkloadProfileContract::bind(&build_plan, web_profile(250))?;
    let first_revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        initial_contract.clone(),
        1,
        first_actor,
        build_plan.accepted_at + ChronoDuration::seconds(1),
    )?;
    let first_write = profile_write(
        &build_plan,
        &first_revision,
        None,
        "accept-workload-profile-1",
    )?;
    let first_idempotency = first_write.idempotency.clone();
    assert!(!profiles.accept(first_write).await?.replayed);
    assert_eq!(
        profiles.replay_acceptance(&first_idempotency).await?,
        Some(first_revision.clone())
    );

    let same_actor_candidate = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        initial_contract.clone(),
        2,
        first_actor,
        first_revision.accepted_at + ChronoDuration::seconds(1),
    )?;
    let adopted = profiles
        .accept(profile_write(
            &build_plan,
            &same_actor_candidate,
            Some(first_revision.id),
            "adopt-workload-profile",
        )?)
        .await?;
    assert!(adopted.replayed);
    assert_eq!(adopted.value, first_revision);

    let second_revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        initial_contract,
        2,
        second_actor,
        first_revision.accepted_at + ChronoDuration::seconds(1),
    )?;
    let second_write = profile_write(
        &build_plan,
        &second_revision,
        Some(first_revision.id),
        "accept-workload-profile-2",
    )?;
    let second_idempotency = second_write.idempotency.clone();
    assert!(!profiles.accept(second_write).await?.replayed);

    let changed_contract = WorkloadProfileContract::bind(&build_plan, web_profile(500))?;
    let third_revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        changed_contract,
        3,
        second_actor,
        second_revision.accepted_at + ChronoDuration::seconds(1),
    )?;
    assert!(
        !profiles
            .accept(profile_write(
                &build_plan,
                &third_revision,
                Some(second_revision.id),
                "accept-workload-profile-3",
            )?)
            .await?
            .replayed
    );

    let restarted = PostgresWorkloadProfileRepository::new(executor.clone());
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
                second_revision.profile_id,
                second_revision.id,
            )
            .await?,
        Some(second_revision.clone())
    );
    assert_eq!(
        restarted
            .find_current(
                organization_id,
                project_id,
                environment_id,
                third_revision.profile_id,
            )
            .await?,
        Some(third_revision.clone())
    );
    assert_eq!(
        restarted
            .list_revisions(
                organization_id,
                project_id,
                environment_id,
                third_revision.profile_id,
                10,
            )
            .await?,
        vec![
            first_revision.clone(),
            second_revision.clone(),
            third_revision.clone()
        ]
    );

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64)>(
                    "select (select count(*) from outbox_events where event_key = ",
                )
                .bind("developer.workload-profile.revision-accepted")
                .append(" and organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(third_revision.profile_id.as_uuid())
                .append("), (select count(*) from audit_records where action = ")
                .bind("developer.workload-profile.revision-accepted")
                .append(" and organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(third_revision.profile_id.as_uuid())
                .append(")"),
            )
            .await?,
        (3, 3)
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64, i64, i64)>(
                    "select (select count(*) from build_runs where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from workloads where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from routes where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from executions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(")"),
            )
            .await?,
        (0, 0, 0, 0),
        "workload profile acceptance must not create runtime owner resources"
    );

    let mutation = database
        .execute(
            sql_query::<()>("update developer_workload_profile_revisions set accepted_at = accepted_at + interval '1 second' where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(third_revision.id.as_uuid()),
        )
        .await
        .expect_err("accepted workload profile mutation must fail");
    assert_eq!(
        database_error_message(&mutation),
        Some("accepted workload profile revisions are immutable")
    );

    let mismatched_plan = database
        .execute(
            clone_revision_insert()
                .bind(Uuid::now_v7())
                .append(", revision_number + 1, build_plan_id, source_revision_id, 'other-root', profile_name, profile_kind, contract_schema, canonical_acl, contract_digest, build_plan_digest, accepted_by, accepted_at + interval '1 second' from developer_workload_profile_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(third_revision.id.as_uuid()),
        )
        .await
        .expect_err("mismatched BuildPlan projection must fail");
    assert_eq!(
        database_error_message(&mismatched_plan),
        Some("accepted workload profile revision does not match its exact BuildPlan")
    );

    let sequence_gap = database
        .execute(
            clone_revision_insert()
                .bind(Uuid::now_v7())
                .append(", revision_number + 2, build_plan_id, source_revision_id, project_root, profile_name, profile_kind, contract_schema, canonical_acl, contract_digest, build_plan_digest, accepted_by, accepted_at + interval '1 second' from developer_workload_profile_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(third_revision.id.as_uuid()),
        )
        .await
        .expect_err("workload profile sequence gap must fail");
    assert_eq!(
        database_error_message(&sequence_gap),
        Some("workload profile revision sequence is not monotonic")
    );

    Ok(())
}

async fn seed_source_revision(
    executor: PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: chrono::DateTime<Utc>,
) -> Result<ExternalSourceRevision, Box<dyn std::error::Error>> {
    let repository = GitRepository::parse(
        GitProvider::Github,
        "https://github.com/a3s-lab/developer-workload-profile-fixture",
    )?;
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
        commit_sha: GitCommitSha::parse("1".repeat(40))?,
        recipe,
        accepted_at: created_at + ChronoDuration::seconds(1),
    })?;
    let request_id = Uuid::now_v7();
    PostgresSourceRevisionRepository::new(executor)
        .accept(AcceptSourceRevision {
            event: SourceRevisionAccepted::envelope(&source_revision, request_id)?,
            revision: source_revision.clone(),
            webhook_delivery: None,
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"
                ),
                "developer-workload-profile-source",
                source_revision.id.as_uuid().as_bytes(),
            )?,
        })
        .await?;
    Ok(source_revision)
}

fn accepted_build_plan(
    source_revision: &ExternalSourceRevision,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
) -> Result<AcceptedBuildPlan, String> {
    let proposal = BuildPlanProposal::from_spec(BuildPlanProposalSpec {
        source: SourceLayoutIdentity::new(
            Sha256Digest::parse(source_revision.source_identity_digest())?,
            source_revision.commit_sha.clone(),
            digest('b'),
        )?,
        detector: BuildPlanDetectorKind::Dockerfile,
        detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
        project_root: ".".into(),
        evidence_path: "Dockerfile".into(),
        evidence_digest: digest('c'),
        recipe: source_revision.recipe.clone(),
    })?;
    AcceptedBuildPlan::accept(
        organization_id,
        project_id,
        environment_id,
        AcceptedBuildPlanContract::from_proposal(source_revision.id, proposal)?,
        actor,
        source_revision.accepted_at + ChronoDuration::seconds(1),
    )
}

fn build_plan_write(plan: &AcceptedBuildPlan, key: &str) -> Result<AcceptBuildPlanWrite, String> {
    let request_id = Uuid::now_v7();
    Ok(AcceptBuildPlanWrite {
        plan: plan.clone(),
        event: BuildPlanAccepted::envelope(plan, request_id)?,
        actor_principal_id: plan.accepted_by,
        request_id,
        idempotency: IdempotencyRequest::new(
            "developer-workload-profile-build-plan",
            key,
            plan.contract.digest().as_str().as_bytes(),
        )?,
    })
}

fn profile_write(
    plan: &AcceptedBuildPlan,
    revision: &AcceptedWorkloadProfileRevision,
    expected_previous_revision_id: Option<WorkloadProfileRevisionId>,
    key: &str,
) -> Result<AcceptWorkloadProfileRevisionWrite, String> {
    let request_id = Uuid::now_v7();
    Ok(AcceptWorkloadProfileRevisionWrite {
        revision: revision.clone(),
        build_plan: plan.clone(),
        expected_previous_revision_id,
        event: WorkloadProfileRevisionAccepted::envelope(revision, request_id)?,
        actor_principal_id: revision.accepted_by,
        request_id,
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/workload-profiles",
                revision.organization_id, revision.project_id, revision.environment_id
            ),
            key,
            revision.contract.digest().as_str().as_bytes(),
        )?,
    })
}

fn web_profile(cpu_millis: u64) -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "api".into(),
        kind: WorkloadProfileKind::Web,
        process: WorkloadProcess {
            command: vec!["/app/server".into()],
            args: vec!["--production".into()],
            working_directory: Some("/app".into()),
            environment: BTreeMap::from([("LOG_LEVEL".into(), "info".into())]),
        },
        secrets: Vec::new(),
        resources: WorkloadProfileResources {
            cpu_millis,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: Some(256 * 1024 * 1024),
            execution_timeout_ms: None,
        },
        ports: vec![WorkloadServicePort {
            name: "http".into(),
            container_port: 8_080,
        }],
        health: Some(WorkloadHttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 5_000,
            timeout_ms: 1_000,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            stabilization_window_ms: 10_000,
        }),
        public_port: Some("http".into()),
        schedule: None,
    }
}

fn clone_revision_insert() -> a3s_orm::SqlQuery<()> {
    sql_query::<()>("insert into developer_workload_profile_revisions (organization_id, project_id, environment_id, profile_id, id, revision_number, build_plan_id, source_revision_id, project_root, profile_name, profile_kind, contract_schema, canonical_acl, contract_digest, build_plan_digest, accepted_by, accepted_at) select organization_id, project_id, environment_id, profile_id, ")
}

async fn seed_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actors: &[PrincipalId],
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'Workload profile tenant', ")
                .bind(format!("workload-profile-{organization_id}"))
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
                .append(", 'Workload profile project', 'workload-profile-project', 1, ")
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
                .append(", 'Workload profile environment', 'workload-profile-environment', 1, ")
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
                    .bind(format!("Workload profile actor {index}"))
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
