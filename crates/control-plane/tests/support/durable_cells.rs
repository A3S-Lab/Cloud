use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_control_plane::modules::artifacts::{BuildRun, PostgresBuildRunRepository};
use a3s_cloud_control_plane::modules::durable_cells::application::{
    CreateDurableCellApplication, CreateDurableCellApplicationHandler, GetDurableCellApplication,
    GetDurableCellApplicationHandler, ListDurableCellApplicationRevisions,
    ListDurableCellApplicationRevisionsHandler, ReviseDurableCellApplication,
    ReviseDurableCellApplicationHandler, StartDurableCellApplication,
    StartDurableCellApplicationHandler, StopDurableCellApplication,
    StopDurableCellApplicationHandler,
};
use a3s_cloud_control_plane::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, CreateDurableCellDeploymentWrite, DurableCellApplication,
    DurableCellApplicationChanged, DurableCellApplicationDefinition,
    DurableCellApplicationDefinitionSpec, DurableCellApplicationDesiredState,
    DurableCellApplicationRecord, DurableCellApplicationRevision, DurableCellClassSpec,
    DurableCellDeployment, DurableCellProjectionIdentity, DurableCellProviderBinding,
    DurableCellRollbackPolicy, DurableCellStateSchema, DurableCellStorageBinding,
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    RequestDurableCellApplicationStateWrite, ReviseDurableCellApplicationWrite,
};
use a3s_cloud_control_plane::modules::durable_cells::{
    PostgresDurableCellApplicationRepository, PostgresDurableCellDeploymentRepository,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::ResourceGrantScope;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    BuildRunId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError, ResourceName,
    Sha256Digest, SourceRevisionId,
};
use chrono::Duration;

pub(super) async fn exercise_durable_cell_application_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("116"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (
            1,
            "immutable Durable Cell applications and revisions".into()
        )
    );
    let deployment_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("117"),
        )
        .await?;
    assert_eq!(
        deployment_migration_state,
        (1, "immutable Durable Cell deployment correlations".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Durable Cell tenant', ")
            .bind(format!("durable-cell-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Durable Cell owner', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", 'Durable Cell project', 'durable-cell-project', 1, ")
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
                .append(", 'Production', 'production', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let initial_build_run_id = insert_queued_build_run(
        &database,
        organization_id,
        project_id,
        environment_id,
        'a',
        created_at,
    )
    .await?;
    let successor_build_run_id = insert_queued_build_run(
        &database,
        organization_id,
        project_id,
        environment_id,
        'b',
        created_at + Duration::milliseconds(1),
    )
    .await?;

    let repository = PostgresDurableCellApplicationRepository::new(executor.clone());
    let application_id = DurableCellApplicationId::new();
    let initial = DurableCellApplicationRevision::initial(
        organization_id,
        project_id,
        environment_id,
        application_id,
        DurableCellApplicationRevisionId::new(),
        definition(initial_build_run_id, 'a', 1)?,
        actor,
        created_at + Duration::seconds(1),
    )?;
    let application = DurableCellApplication::create(
        application_id,
        ResourceName::parse("Tenant counters")?,
        &initial,
    )?;
    let initial_record = DurableCellApplicationRecord::new(application.clone(), initial.clone())?;
    let create_request_id = Uuid::now_v7();
    let create_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications"
        ),
        "durable-cell-create",
        initial.definition.canonical_acl().as_bytes(),
    )?;
    let create = CreateDurableCellApplicationWrite {
        record: initial_record.clone(),
        event: DurableCellApplicationChanged::created(&application, &initial, create_request_id)?,
        actor_principal_id: actor,
        request_id: create_request_id,
        idempotency: create_idempotency.clone(),
    };
    assert!(!repository.create(create.clone()).await?.replayed);
    let create_replay = repository.create(create).await?;
    assert!(create_replay.replayed);
    assert_eq!(create_replay.value, initial_record);

    let conflicting_request_id = Uuid::now_v7();
    assert_eq!(
        repository
            .create(CreateDurableCellApplicationWrite {
                record: initial_record.clone(),
                event: DurableCellApplicationChanged::created(
                    &application,
                    &initial,
                    conflicting_request_id,
                )?,
                actor_principal_id: actor,
                request_id: conflicting_request_id,
                idempotency: IdempotencyRequest::new(
                    create_idempotency.scope.clone(),
                    create_idempotency.key.clone(),
                    b"different Durable Cell request",
                )?,
            })
            .await,
        Err(RepositoryError::IdempotencyConflict)
    );

    let stopped = application.request_state(
        1,
        DurableCellApplicationDesiredState::Stopped,
        initial.created_at + Duration::seconds(1),
    )?;
    let stopped_record = DurableCellApplicationRecord::new(stopped.clone(), initial.clone())?;
    let stop_request_id = Uuid::now_v7();
    let stop = RequestDurableCellApplicationStateWrite {
        record: stopped_record.clone(),
        expected_version: 1,
        event: DurableCellApplicationChanged::state_requested(&stopped, &initial, stop_request_id)?,
        actor_principal_id: actor,
        request_id: stop_request_id,
        idempotency: IdempotencyRequest::new(
            format!("organizations/{organization_id}/durable-cell-applications/{application_id}"),
            "durable-cell-stop",
            b"stopped",
        )?,
    };
    assert!(!repository.request_state(stop.clone()).await?.replayed);

    let successor = DurableCellApplicationRevision::successor(
        &initial,
        DurableCellApplicationRevisionId::new(),
        definition(successor_build_run_id, 'b', 2)?,
        actor,
        initial.created_at + Duration::seconds(2),
    )?;
    let revised = stopped.advance(2, &successor)?;
    let revised_record = DurableCellApplicationRecord::new(revised.clone(), successor.clone())?;
    let revise_request_id = Uuid::now_v7();
    let revise = ReviseDurableCellApplicationWrite {
        record: revised_record.clone(),
        expected_version: 2,
        event: DurableCellApplicationChanged::revised(
            &revised,
            &successor,
            revise_request_id,
        )?,
        actor_principal_id: actor,
        request_id: revise_request_id,
        idempotency: IdempotencyRequest::new(
            format!("organizations/{organization_id}/durable-cell-applications/{application_id}/revisions"),
            "durable-cell-revise",
            successor.definition.canonical_acl().as_bytes(),
        )?,
    };
    assert!(!repository.revise(revise.clone()).await?.replayed);
    assert!(repository.revise(revise).await?.replayed);

    let historical_stop = repository.request_state(stop).await?;
    assert!(historical_stop.replayed);
    assert_eq!(historical_stop.value, stopped_record);
    assert_eq!(
        repository
            .find(organization_id, project_id, environment_id, application_id)
            .await?,
        Some(revised.clone())
    );
    assert!(repository
        .find(
            organization_id,
            project_id,
            EnvironmentId::new(),
            application_id,
        )
        .await?
        .is_none());
    assert_eq!(
        repository
            .list_revisions(
                organization_id,
                project_id,
                environment_id,
                application_id,
                10,
            )
            .await?,
        vec![successor.clone(), initial.clone()]
    );

    let projection = DurableCellProjectionIdentity::for_current_revision(&revised, &successor)?;
    let deployment_request_id = Uuid::now_v7();
    let deployment = DurableCellDeployment::bind(
        projection.clone(),
        DurableCellStorageBinding {
            organization_id,
            project_id,
            environment_id,
            application_id,
            application_revision_id: successor.id,
            application_revision_number: successor.revision_number,
            application_definition_digest: successor.definition.digest().clone(),
            storage_namespace_id: projection.storage_namespace_id,
            credential_binding_generation: 1,
            credential_binding_digest: digest('1'),
            provider_profile_digest: digest('2'),
            retention_policy_digest: digest('3'),
        },
        DurableCellProviderBinding {
            application_id,
            application_revision_id: successor.id,
            application_revision_number: successor.revision_number,
            application_definition_digest: successor.definition.digest().clone(),
            workload_id: projection.workload_id,
            workload_revision_id: projection.workload_revision_id,
            workload_generation: 1,
            service_profile_digest: successor.definition.spec().service_profile_digest.clone(),
            service_template_digest: digest('4'),
            provider_artifact_digest: digest('5'),
        },
        digest('6'),
        actor,
        deployment_request_id,
        successor.created_at + Duration::seconds(1),
    )?;
    let deployment_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/durable-cell-applications/{application_id}/revisions/{}/deployment",
            successor.id
        ),
        "durable-cell-deployment-correlation",
        b"exact Durable Cell deployment correlation",
    )?;
    let deployment_repository = PostgresDurableCellDeploymentRepository::new(executor.clone());
    assert!(deployment_repository
        .replay(&deployment_idempotency)
        .await?
        .is_none());
    let deployment_write = CreateDurableCellDeploymentWrite {
        deployment: deployment.clone(),
        idempotency: deployment_idempotency.clone(),
    };
    assert!(
        !deployment_repository
            .create(deployment_write.clone())
            .await?
            .replayed
    );
    let deployment_replay = deployment_repository.create(deployment_write).await?;
    assert!(deployment_replay.replayed);
    assert_eq!(deployment_replay.value, deployment);
    assert_eq!(
        deployment_repository
            .replay(&deployment_idempotency)
            .await?,
        Some(deployment.clone())
    );
    assert_eq!(
        deployment_repository
            .find(
                organization_id,
                project_id,
                environment_id,
                application_id,
                successor.id,
            )
            .await?,
        Some(deployment.clone())
    );
    assert!(deployment_repository
        .find(
            organization_id,
            project_id,
            EnvironmentId::new(),
            application_id,
            successor.id,
        )
        .await?
        .is_none());
    assert_eq!(
        deployment_repository
            .create(CreateDurableCellDeploymentWrite {
                deployment: deployment.clone(),
                idempotency: IdempotencyRequest::new(
                    deployment_idempotency.scope.clone(),
                    deployment_idempotency.key.clone(),
                    b"different Durable Cell deployment correlation",
                )?,
            })
            .await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert!(matches!(
        deployment_repository
            .create(CreateDurableCellDeploymentWrite {
                deployment: deployment.clone(),
                idempotency: IdempotencyRequest::new(
                    deployment_idempotency.scope.clone(),
                    "another Durable Cell deployment request",
                    b"exact Durable Cell deployment correlation",
                )?,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update durable_cell_deployments set requested_at = requested_at + interval '1 second' where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and application_revision_id = ")
                    .bind(successor.id.as_uuid()),
            )
            .await,
        "mutating an immutable Durable Cell deployment correlation",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from durable_cell_deployments where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and application_revision_id = ")
                    .bind(successor.id.as_uuid()),
            )
            .await,
        "deleting an immutable Durable Cell deployment correlation",
    );
    let deployment_evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>("select (select count(*) from durable_cell_deployments where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append("), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(application_id.as_uuid())
                .append(" and action = 'durable-cell.deployment.requested'), (select count(*) from idempotency_records where scope_key = ")
                .bind(deployment_idempotency.scope.clone())
                .append(" and idempotency_key = ")
                .bind(deployment_idempotency.key.clone())
                .append(")"),
        )
        .await?;
    assert_eq!(deployment_evidence, (1, 1, 1));

    let cqrs_initial_build_run_id = insert_queued_build_run(
        &database,
        organization_id,
        project_id,
        environment_id,
        'c',
        created_at + Duration::milliseconds(2),
    )
    .await?;
    let cqrs_successor_build_run_id = insert_queued_build_run(
        &database,
        organization_id,
        project_id,
        environment_id,
        'd',
        created_at + Duration::milliseconds(3),
    )
    .await?;
    let cqrs_repository = Arc::new(PostgresDurableCellApplicationRepository::new(
        executor.clone(),
    ));
    let cqrs_builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let cqrs_projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let create_handler = CreateDurableCellApplicationHandler::new(
        cqrs_projects,
        cqrs_repository.clone(),
        cqrs_builds.clone(),
    );
    let create_command = CreateDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        name: "CQRS counters".into(),
        definition_acl: definition(cqrs_initial_build_run_id, 'c', 1)?
            .canonical_acl()
            .to_owned(),
        actor_principal_id: actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "durable-cell-cqrs-create".into(),
        request_id: Uuid::now_v7(),
    };
    let cqrs_created = create_handler
        .execute(create_command.clone(), cqrs_context())
        .await??;
    assert!(!cqrs_created.replayed);
    let denied_replay = create_handler
        .execute(
            CreateDurableCellApplication {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..create_command.clone()
            },
            cqrs_context(),
        )
        .await?;
    assert!(matches!(denied_replay, Err(ApplicationError::NotFound(_))));
    assert!(
        create_handler
            .execute(create_command, cqrs_context())
            .await??
            .replayed
    );

    let revise_handler =
        ReviseDurableCellApplicationHandler::new(cqrs_repository.clone(), cqrs_builds);
    let cqrs_revised = revise_handler
        .execute(
            ReviseDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: cqrs_created.record.application.id,
                expected_version: 1,
                definition_acl: definition(cqrs_successor_build_run_id, 'd', 2)?
                    .canonical_acl()
                    .to_owned(),
                actor_principal_id: actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "durable-cell-cqrs-revise".into(),
                request_id: Uuid::now_v7(),
            },
            cqrs_context(),
        )
        .await??;
    let stop_handler = StopDurableCellApplicationHandler::new(cqrs_repository.clone());
    let cqrs_stopped = stop_handler
        .execute(
            StopDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: cqrs_created.record.application.id,
                expected_version: 2,
                actor_principal_id: actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "durable-cell-cqrs-stop".into(),
                request_id: Uuid::now_v7(),
            },
            cqrs_context(),
        )
        .await??;
    assert_eq!(
        cqrs_stopped.record.application.desired_state,
        DurableCellApplicationDesiredState::Stopped
    );
    let start_handler = StartDurableCellApplicationHandler::new(cqrs_repository.clone());
    let cqrs_started = start_handler
        .execute(
            StartDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: cqrs_created.record.application.id,
                expected_version: 3,
                actor_principal_id: actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "durable-cell-cqrs-start".into(),
                request_id: Uuid::now_v7(),
            },
            cqrs_context(),
        )
        .await??;
    assert_eq!(cqrs_started.record.application.aggregate_version, 4);
    assert_eq!(
        GetDurableCellApplicationHandler::new(cqrs_repository.clone())
            .execute(
                GetDurableCellApplication {
                    organization_id,
                    project_id,
                    environment_id,
                    application_id: cqrs_created.record.application.id,
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                },
                cqrs_context(),
            )
            .await??,
        cqrs_started.record
    );
    assert_eq!(
        ListDurableCellApplicationRevisionsHandler::new(cqrs_repository)
            .execute(
                ListDurableCellApplicationRevisions {
                    organization_id,
                    project_id,
                    environment_id,
                    application_id: cqrs_created.record.application.id,
                    limit: 10,
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                },
                cqrs_context(),
            )
            .await??,
        vec![cqrs_revised.record.revision, cqrs_created.record.revision]
    );

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update durable_cell_application_revisions set canonical_acl = 'tampered' where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and id = ")
                    .bind(initial.id.as_uuid()),
            )
            .await,
        "mutating an immutable Durable Cell revision",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update durable_cell_applications set aggregate_version = aggregate_version + 2 where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(application_id.as_uuid()),
            )
            .await,
        "skipping a Durable Cell application version",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into durable_cell_application_revisions (organization_id, project_id, environment_id, application_id, id, revision_number, parent_revision_id, parent_definition_digest, definition_schema, canonical_acl, definition_digest, build_run_id, created_by, created_at) values (")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(project_id.as_uuid())
                    .append(", ")
                    .bind(environment_id.as_uuid())
                    .append(", ")
                    .bind(application_id.as_uuid())
                    .append(", ")
                    .bind(DurableCellApplicationRevisionId::new().as_uuid())
                    .append(", 3, ")
                    .bind(successor.id.as_uuid())
                    .append(", ")
                    .bind(successor.definition.digest().as_str())
                    .append(", 'cloud.durable-cell.application.v1', ")
                    .bind(successor.definition.canonical_acl())
                    .append(", ")
                    .bind(successor.definition.digest().as_str())
                    .append(", ")
                    .bind(successor_build_run_id.as_uuid())
                    .append(", ")
                    .bind(actor.as_uuid())
                    .append(", ")
                    .bind(successor.created_at + Duration::seconds(1))
                    .append(")"),
            )
            .await,
        "inserting a no-op Durable Cell revision",
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>("select (select count(*) from durable_cell_applications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from durable_cell_application_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key like 'durable-cell.application.%'), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action like 'durable-cell.application.%'), (select count(*) from idempotency_records where scope_key like ")
                .bind(format!("organizations/{organization_id}/%durable-cell%"))
                .append(")"),
        )
        .await?;
    assert_eq!(evidence, (2, 4, 7, 7, 8));
    let duplicated_authority = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from idempotency_records where response::text like '%canonical_acl%' or response::text like '%worker.mjs%'",
            ),
        )
        .await?;
    assert_eq!(duplicated_authority, 0);
    let forbidden_tables = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from (values (to_regclass('public.cells')), (to_regclass('public.cell_ownership')), (to_regclass('public.durable_cell_queue'))) as forbidden(relation) where relation is not null",
        ))
        .await?;
    assert_eq!(forbidden_tables, 0);
    Ok(())
}

async fn insert_queued_build_run(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    marker: char,
    accepted_at: chrono::DateTime<Utc>,
) -> Result<BuildRunId, Box<dyn std::error::Error>> {
    let source_revision_id = SourceRevisionId::new();
    let marker_text = marker.to_string();
    let recipe = serde_json::json!({
        "schema": "a3s.cloud.build-recipe.v1",
        "kind": "dockerfile",
        "contextPath": ".",
        "dockerfilePath": "Dockerfile",
        "target": null,
        "platforms": ["linux/amd64"],
    });
    database
        .execute(
            sql_query::<()>("insert into external_source_revisions (organization_id, project_id, environment_id, id, repository_provider, repository_url, repository_identity, commit_sha, recipe, recipe_digest, aggregate_version, accepted_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", ")
                .bind(source_revision_id.as_uuid())
                .append(", 'github', ")
                .bind(format!("https://github.com/a3s-lab/cell-fixture-{marker}"))
                .append(", ")
                .bind(format!("github:github.com/a3s-lab/cell-fixture-{marker}"))
                .append(", ")
                .bind(marker_text.repeat(40))
                .append(", ")
                .bind(recipe)
                .append(", ")
                .bind(format!("sha256:{}", marker_text.repeat(64)))
                .append(", 1, ")
                .bind(accepted_at)
                .append(")"),
        )
        .await?;
    let build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        accepted_at,
    );
    database
        .execute(
            sql_query::<()>("insert into build_runs (organization_id, subject_kind, project_id, environment_id, source_revision_id, id, attempt, retry_of_build_run_id, operation_id, status, evidence_required, aggregate_version, requested_at, updated_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'external_source_revision', ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", ")
                .bind(source_revision_id.as_uuid())
                .append(", ")
                .bind(build.id.as_uuid())
                .append(", 1, null, ")
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
    Ok(build.id)
}

fn definition(
    build_run_id: BuildRunId,
    marker: char,
    write_version: u64,
) -> Result<DurableCellApplicationDefinition, String> {
    DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
        build_run_id,
        bundle_digest: digest(marker),
        bundle_size_bytes: 1024,
        main_module: "worker.mjs".into(),
        compatibility_date: "2026-08-16".into(),
        compatibility_flags: Vec::new(),
        cell_classes: vec![DurableCellClassSpec {
            name: "Counter".into(),
            state_schema: DurableCellStateSchema {
                minimum_readable_version: 1,
                maximum_readable_version: 2,
                write_version,
            },
        }],
        service_profile_digest: digest('f'),
        rollback_policy: DurableCellRollbackPolicy::Compatible,
    })
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "PostgreSQL accepted {label}");
}

fn cqrs_context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
