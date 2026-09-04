use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{artifact_uri, DURABLE_CELL_BUNDLE_MEDIA_TYPE};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildRun, IBuildRunRepository, OciDescriptor, OciPublicationTarget,
    PostgresBuildRunRepository, PublishedOciArtifact, ValidatedOciBuildOutput,
};
use a3s_cloud_control_plane::modules::data::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceProviderProfile, ObjectNamespaceRetentionPolicy,
    ObjectNamespaceRetentionPolicySpec,
};
use a3s_cloud_control_plane::modules::durable_cells::application::{
    compose_pinned_celld_service_process, CreateDurableCellApplication,
    CreateDurableCellApplicationHandler, DeployDurableCellApplication,
    DeployDurableCellApplicationHandler, DurableCellStorageProviderProfileProjection,
    GetDurableCellApplication, GetDurableCellApplicationHandler,
    ListDurableCellApplicationRevisions, ListDurableCellApplicationRevisionsHandler,
    ReviseDurableCellApplication, ReviseDurableCellApplicationHandler, StartDurableCellApplication,
    StartDurableCellApplicationHandler, StopDurableCellApplication,
    StopDurableCellApplicationHandler,
};
use a3s_cloud_control_plane::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, CreateDurableCellDeploymentWrite, DurableCellApplication,
    DurableCellApplicationChanged, DurableCellApplicationDefinition,
    DurableCellApplicationDefinitionSpec, DurableCellApplicationDesiredState,
    DurableCellApplicationRecord, DurableCellApplicationRevision, DurableCellClassSpec,
    DurableCellDeployment, DurableCellDeploymentRequest, DurableCellProjectionIdentity,
    DurableCellProviderBinding, DurableCellPublisherProfile, DurableCellRollbackPolicy,
    DurableCellServiceProfile, DurableCellStateSchema, DurableCellStorageBinding,
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    RequestDurableCellApplicationStateWrite, ReviseDurableCellApplicationWrite,
};
use a3s_cloud_control_plane::modules::durable_cells::{
    ArtifactsDurableCellBuildArtifactAdapter, DataDurableCellStorageAdapter,
    FleetDurableCellNodePoolAdapter, PostgresDurableCellApplicationRepository,
    PostgresDurableCellDeploymentRepository, SecretsDurableCellBindingAdapter,
    WorkloadsDurableCellWorkloadAdapter,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::NodeCapabilities;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::ResourceGrantScope;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::secrets::exact_secret_version_access;
use a3s_cloud_control_plane::modules::secrets::{
    CreateSecretWrite, EncryptedSecretValue, ISecretRepository, PostgresSecretRepository, Secret,
    SecretChanged,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    BuildRunId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, ResourceName, SecretId, SecretVersionReference, Sha256Digest,
    SourceRevisionId, StorageNamespaceId,
};
use a3s_cloud_control_plane::modules::sources::domain::BuildPlatform;
use a3s_cloud_control_plane::modules::workloads::{
    HttpHealthCheck, IWorkloadReplicaRetirementRepository, IWorkloadRepository, OciArtifact,
    PostgresWorkloadRepository, ReplicaRetirementCompletion, SecretBinding, SecretBindingTarget,
    ServicePort, ServiceResources, ServiceTemplate, WorkloadReplicaLifecycle,
};
use chrono::Duration;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration as StdDuration;

const DURABLE_CELL_SERVICE_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/cell0.3/celld-v0.2.1-service-profile.acl"
));
const OBJECT_NAMESPACE_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/s0.1/object-namespace-provider-profile.acl"
));
const PROJECTION_CRASH_PROBE_TEST: &str = "durable_cell_projection_process_death_probe";
const PROJECTION_CRASH_PARENT_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_PARENT";
const PROJECTION_CRASH_POSTGRES_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_POSTGRES_URL";
const PROJECTION_CRASH_ORGANIZATION_ENV: &str =
    "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_ORGANIZATION_ID";
const PROJECTION_CRASH_PROJECT_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_PROJECT_ID";
const PROJECTION_CRASH_ENVIRONMENT_ENV: &str =
    "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_ENVIRONMENT_ID";
const PROJECTION_CRASH_APPLICATION_ENV: &str =
    "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_APPLICATION_ID";
const PROJECTION_CRASH_REVISION_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_REVISION_ID";
const PROJECTION_CRASH_NAMESPACE_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_NAMESPACE_ID";
const PROJECTION_CRASH_ACCESS_KEY_ENV: &str =
    "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_ACCESS_KEY_ID";
const PROJECTION_CRASH_SECRET_KEY_ENV: &str =
    "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_SECRET_KEY_ID";
const PROJECTION_CRASH_ACTOR_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_ACTOR_ID";
const PROJECTION_CRASH_REQUEST_ENV: &str = "A3S_CLOUD_DURABLE_CELL_PROJECTION_CRASH_REQUEST_ID";

#[derive(Clone)]
struct ProjectionCrashInput {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    application_revision_id: DurableCellApplicationRevisionId,
    storage_namespace_id: StorageNamespaceId,
    access_key_id: SecretVersionReference,
    secret_access_key: SecretVersionReference,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
}

#[derive(Clone, Copy)]
struct DurableCellTenantFixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
}

pub(super) async fn exercise_durable_cell_application_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
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

    let DurableCellTenantFixture {
        organization_id,
        project_id,
        environment_id,
        actor,
        created_at,
    } = seed_durable_cell_tenant(&database).await?;

    let initial_build_run_id = insert_typed_build_run(
        &database,
        &executor,
        organization_id,
        project_id,
        environment_id,
        'a',
        created_at,
    )
    .await?;
    let successor_build_run_id = insert_typed_build_run(
        &database,
        &executor,
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
    let storage_provider_profile =
        ObjectNamespaceProviderProfile::parse_acl(OBJECT_NAMESPACE_PROVIDER_PROFILE_ACL)?;
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
            provider_profile_digest: storage_provider_profile.digest().clone(),
            retention_policy_digest: digest('3'),
        },
        Some(storage_provider_profile.canonical_acl()),
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
        DurableCellDeploymentRequest {
            requested_by: actor,
            request_id: deployment_request_id,
            requested_at: successor.created_at + Duration::seconds(1),
        },
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

    let cqrs_initial_build_run_id = insert_typed_build_run(
        &database,
        &executor,
        organization_id,
        project_id,
        environment_id,
        'c',
        created_at + Duration::milliseconds(2),
    )
    .await?;
    let cqrs_successor_build_run_id = insert_typed_build_run(
        &database,
        &executor,
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
    let cqrs_workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let cqrs_workload_port = Arc::new(WorkloadsDurableCellWorkloadAdapter::new(
        cqrs_repository.clone(),
        cqrs_workloads.clone(),
        cqrs_workloads.clone(),
    ));
    let cqrs_builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let cqrs_build_artifacts = Arc::new(ArtifactsDurableCellBuildArtifactAdapter::new(
        cqrs_builds.clone(),
    ));
    let cqrs_projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let create_handler = CreateDurableCellApplicationHandler::new(
        cqrs_projects,
        cqrs_repository.clone(),
        cqrs_build_artifacts.clone(),
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
        ReviseDurableCellApplicationHandler::new(cqrs_repository.clone(), cqrs_build_artifacts);
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
    let stop_handler =
        StopDurableCellApplicationHandler::new(cqrs_repository.clone(), cqrs_workload_port.clone());
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
    let start_handler =
        StartDurableCellApplicationHandler::new(cqrs_repository.clone(), cqrs_workload_port);
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

pub(super) async fn exercise_durable_cell_projection_process_death(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let tenant = seed_durable_cell_tenant(&database).await?;
    let profile = DurableCellServiceProfile::parse_acl(DURABLE_CELL_SERVICE_PROFILE_ACL)?;
    let build_run_id = insert_typed_build_run(
        &database,
        &executor,
        tenant.organization_id,
        tenant.project_id,
        tenant.environment_id,
        '6',
        tenant.created_at,
    )
    .await?;
    let application_id = DurableCellApplicationId::new();
    let revision = DurableCellApplicationRevision::initial(
        tenant.organization_id,
        tenant.project_id,
        tenant.environment_id,
        application_id,
        DurableCellApplicationRevisionId::new(),
        definition_with_service_profile(build_run_id, '6', 1, profile.digest().clone())?,
        tenant.actor,
        tenant.created_at + Duration::seconds(1),
    )?;
    let application = DurableCellApplication::create(
        application_id,
        ResourceName::parse("Process-death counters")?,
        &revision,
    )?;
    let record = DurableCellApplicationRecord::new(application.clone(), revision.clone())?;
    let application_request_id = Uuid::now_v7();
    let applications = PostgresDurableCellApplicationRepository::new(executor.clone());
    let application_write = applications
        .create(CreateDurableCellApplicationWrite {
            event: DurableCellApplicationChanged::created(
                &application,
                &revision,
                application_request_id,
            )?,
            actor_principal_id: tenant.actor,
            request_id: application_request_id,
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/durable-cell-applications",
                    tenant.organization_id, tenant.project_id, tenant.environment_id,
                ),
                "durable-cell-c6-application",
                revision.definition.canonical_acl().as_bytes(),
            )?,
            record,
        })
        .await?;
    assert!(!application_write.replayed);

    let projection = DurableCellProjectionIdentity::for_current_revision(&application, &revision)?;
    let secrets = PostgresSecretRepository::new(executor.clone());
    let access_key_id = create_postgres_secret(
        &secrets,
        tenant,
        "C6 S0 access key",
        "encrypted-c6-access-key",
    )
    .await?;
    let secret_access_key = create_postgres_secret(
        &secrets,
        tenant,
        "C6 S0 secret key",
        "encrypted-c6-secret-key",
    )
    .await?;
    let input = ProjectionCrashInput {
        organization_id: tenant.organization_id,
        project_id: tenant.project_id,
        environment_id: tenant.environment_id,
        application_id,
        application_revision_id: revision.id,
        storage_namespace_id: projection.storage_namespace_id,
        access_key_id,
        secret_access_key,
        actor_principal_id: tenant.actor,
        request_id: Uuid::now_v7(),
    };
    let command = projection_deployment_command(&input)?;

    let lock_client = executor.pool().get().await?;
    lock_client
        .batch_execute("begin; lock table workloads in share mode")
        .await?;
    let crash_result =
        kill_projection_after_correlation(&database, &postgres_url, &input, &projection).await;
    let release_result = lock_client.batch_execute("rollback").await;
    match (crash_result, release_result) {
        (Ok(()), Ok(())) => {}
        (Err(crash_error), Ok(())) => return Err(crash_error),
        (Ok(()), Err(release_error)) => return Err(release_error.into()),
        (Err(crash_error), Err(release_error)) => {
            return Err(format!(
                "Durable Cell projection crash gate failed: {crash_error}; Workloads lock release also failed: {release_error}"
            )
            .into())
        }
    }

    let committed_boundary = projection_boundary_evidence(&database, &input, &projection).await?;
    assert_eq!(committed_boundary, (1, 0, 0, 0, 0));

    let restarted_executor = PostgresExecutor::connect_no_tls(&postgres_url, 8)?;
    let restarted_workloads = Arc::new(PostgresWorkloadRepository::new(restarted_executor.clone()));
    let handler =
        projection_deployment_handler(&restarted_executor, Arc::clone(&restarted_workloads));
    let recovered = handler.execute(command.clone(), cqrs_context()).await??;
    assert!(recovered.replayed);
    assert!(!recovered.workload.replayed);
    assert_eq!(recovered.correlation.projection, projection);
    assert_eq!(recovered.workload.workload_id, projection.workload_id);
    assert_eq!(
        recovered.workload.revision_id,
        projection.workload_revision_id
    );
    assert_eq!(recovered.workload.deployment_id, projection.deployment_id);
    assert_eq!(recovered.workload.operation_id, projection.operation_id);

    let exact_replay = handler.execute(command, cqrs_context()).await??;
    assert!(exact_replay.replayed);
    assert_eq!(exact_replay.correlation, recovered.correlation);
    assert_eq!(exact_replay.workload, recovered.workload);
    let stored_control = restarted_workloads
        .find_workload_control(tenant.organization_id, projection.workload_id)
        .await?;
    let owner = stored_control
        .spec
        .managed_owner
        .ok_or("recovered Durable Cell Workload omitted its managed owner")?;
    assert_eq!(owner.kind().as_str(), "durable-cell.application");
    assert_eq!(owner.owner_id(), application_id.as_uuid());
    assert_eq!(owner.owner_generation(), revision.revision_number);
    assert_eq!(
        owner.owner_spec_digest(),
        revision.definition.digest().as_str()
    );

    let restarted_database = Database::new(PostgresDialect, restarted_executor.clone());
    let recovered_boundary =
        projection_boundary_evidence(&restarted_database, &input, &projection).await?;
    assert_eq!(recovered_boundary, (1, 1, 1, 1, 1));
    let recovered_evidence = restarted_database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from workload_controls where organization_id = ",
            )
            .bind(tenant.organization_id.as_uuid())
            .append(" and workload_id = ")
            .bind(projection.workload_id.as_uuid())
            .append("), (select count(*) from workload_replicas where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and workload_id = ")
            .bind(projection.workload_id.as_uuid())
            .append("), (select count(*) from workload_replica_members where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and workload_id = ")
            .bind(projection.workload_id.as_uuid())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(projection.deployment_id.as_uuid())
            .append("), (select count(*) from audit_records where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(application_id.as_uuid())
            .append(" and action = 'durable-cell.deployment.requested'), (select count(*) from idempotency_records where scope_key like ")
            .bind(format!(
                "organizations/{}/durable-cell-applications/{}/revisions/{}/%",
                tenant.organization_id, application_id, revision.id,
            ))
            .append(")"),
        )
        .await?;
    assert_eq!(recovered_evidence, (1, 1, 1, 1, 1, 2));

    // Commit only the Durable Cell state mutation, then reconstruct the
    // application composition before Workloads sees it. This is the C6b
    // counterpart to the actual process kill above: the sole Workloads
    // transaction must recover stop, cleanup, and restart without another
    // lifecycle row or controller.
    let state_applications =
        PostgresDurableCellApplicationRepository::new(restarted_executor.clone());
    let stored_application = state_applications
        .find(
            tenant.organization_id,
            tenant.project_id,
            tenant.environment_id,
            application_id,
        )
        .await?
        .ok_or("recovered Durable Cell application disappeared")?;
    let stopped_application = stored_application.request_state(
        stored_application.aggregate_version,
        DurableCellApplicationDesiredState::Stopped,
        Utc::now(),
    )?;
    let stopped_record =
        DurableCellApplicationRecord::new(stopped_application.clone(), revision.clone())?;
    let stop_key = "durable-cell-c6-stop";
    let stop_request_id = Uuid::now_v7();
    state_applications
        .request_state(RequestDurableCellApplicationStateWrite {
            event: DurableCellApplicationChanged::state_requested(
                &stopped_application,
                &revision,
                stop_request_id,
            )?,
            record: stopped_record.clone(),
            expected_version: stored_application.aggregate_version,
            actor_principal_id: tenant.actor,
            request_id: stop_request_id,
            idempotency: durable_cell_state_idempotency(
                &stopped_record,
                stored_application.aggregate_version,
                stop_key,
            )?,
        })
        .await?;
    let pre_recovery_control = restarted_workloads
        .find_workload_control(tenant.organization_id, projection.workload_id)
        .await?;
    assert_eq!(
        pre_recovery_control
            .spec
            .placement_policy
            .desired_replicas(),
        1
    );

    let lifecycle_executor = PostgresExecutor::connect_no_tls(&postgres_url, 8)?;
    let lifecycle_applications = Arc::new(PostgresDurableCellApplicationRepository::new(
        lifecycle_executor.clone(),
    ));
    let lifecycle_workloads = Arc::new(PostgresWorkloadRepository::new(lifecycle_executor.clone()));
    let lifecycle_workload_port = Arc::new(WorkloadsDurableCellWorkloadAdapter::new(
        lifecycle_applications.clone(),
        lifecycle_workloads.clone(),
        lifecycle_workloads.clone(),
    ));
    let stop_command = StopDurableCellApplication {
        organization_id: tenant.organization_id,
        project_id: tenant.project_id,
        environment_id: tenant.environment_id,
        application_id,
        expected_version: stored_application.aggregate_version,
        actor_principal_id: tenant.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: stop_key.into(),
        request_id: stop_request_id,
    };
    let stop_handler = StopDurableCellApplicationHandler::new(
        lifecycle_applications.clone(),
        lifecycle_workload_port.clone(),
    );
    let recovered_stop = stop_handler
        .execute(stop_command.clone(), cqrs_context())
        .await??;
    assert!(recovered_stop.replayed);
    assert_eq!(recovered_stop.record, stopped_record);
    let stopped_control = lifecycle_workloads
        .find_workload_control(tenant.organization_id, projection.workload_id)
        .await?;
    assert_eq!(stopped_control.spec.placement_policy.desired_replicas(), 0);
    let stopped_replicas = lifecycle_workloads
        .list_workload_replicas(tenant.organization_id, projection.workload_id)
        .await?;
    assert_eq!(stopped_replicas.len(), 1);
    assert_eq!(
        stopped_replicas[0].lifecycle,
        WorkloadReplicaLifecycle::Retiring
    );
    assert!(
        stop_handler
            .execute(stop_command, cqrs_context())
            .await??
            .replayed
    );

    let mut retirements = lifecycle_workloads.pending_replica_retirements(10).await?;
    assert_eq!(retirements.len(), 1);
    let retirement = retirements.remove(0);
    assert!(retirement.member.node_id.is_none());
    assert!(retirement
        .deployment
        .as_ref()
        .is_some_and(|deployment| deployment.command_id.is_none()));
    let retired = lifecycle_workloads
        .complete_replica_retirement(ReplicaRetirementCompletion {
            organization_id: tenant.organization_id,
            workload_id: projection.workload_id,
            replica_id: retirement.replica.id,
            replica_generation: retirement.replica.generation,
            expected_replica_version: retirement.replica.aggregate_version,
            member_id: retirement.member.id,
            expected_member_version: retirement.member.aggregate_version,
            fenced_node_id: None,
            completed_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
        })
        .await?;
    assert_eq!(retired.value.lifecycle, WorkloadReplicaLifecycle::Retired);

    let start_command = StartDurableCellApplication {
        organization_id: tenant.organization_id,
        project_id: tenant.project_id,
        environment_id: tenant.environment_id,
        application_id,
        expected_version: stopped_application.aggregate_version,
        actor_principal_id: tenant.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "durable-cell-c6-restart".into(),
        request_id: Uuid::now_v7(),
    };
    let start_handler =
        StartDurableCellApplicationHandler::new(lifecycle_applications, lifecycle_workload_port);
    let restarted = start_handler
        .execute(start_command.clone(), cqrs_context())
        .await??;
    assert!(!restarted.replayed);
    assert_eq!(
        restarted.record.application.desired_state,
        DurableCellApplicationDesiredState::Running
    );
    assert!(
        start_handler
            .execute(start_command, cqrs_context())
            .await??
            .replayed
    );
    let restarted_control = lifecycle_workloads
        .find_workload_control(tenant.organization_id, projection.workload_id)
        .await?;
    assert_eq!(
        restarted_control.spec.placement_policy.desired_replicas(),
        1
    );
    assert_eq!(restarted_control.spec.placement_policy.generation(), 3);
    let restarted_replicas = lifecycle_workloads
        .list_workload_replicas(tenant.organization_id, projection.workload_id)
        .await?;
    assert_eq!(restarted_replicas.len(), 1);
    assert_eq!(restarted_replicas[0].id, retirement.replica.id);
    assert_eq!(
        restarted_replicas[0].lifecycle,
        WorkloadReplicaLifecycle::Desired
    );

    let lifecycle_database = Database::new(PostgresDialect, lifecycle_executor);
    let lifecycle_evidence = lifecycle_database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from outbox_events where organization_id = ",
            )
            .bind(tenant.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(projection.workload_id.as_uuid())
            .append(" and event_key = 'workload.replica-set.reconfigured'), (select count(*) from outbox_events where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(retirement.replica.id.as_uuid())
            .append(" and event_key = 'workload.replica.retired'), (select count(*) from idempotency_records where scope_key = ")
            .bind(format!(
                "organizations/{}/durable-cell-applications/{}/managed-workload-replica-set",
                tenant.organization_id, application_id,
            ))
            .append("), (select count(*) from workload_replicas where organization_id = ")
            .bind(tenant.organization_id.as_uuid())
            .append(" and workload_id = ")
            .bind(projection.workload_id.as_uuid())
            .append(")"),
        )
        .await?;
    assert_eq!(lifecycle_evidence, (2, 1, 2, 1));
    Ok(())
}

pub(super) async fn run_durable_cell_projection_crash_probe(
) -> Result<(), Box<dyn std::error::Error>> {
    if required_projection_probe_environment(PROJECTION_CRASH_PARENT_ENV)? != "1" {
        return Err("Durable Cell projection crash probe requires its private marker".into());
    }
    let postgres_url = required_projection_probe_environment(PROJECTION_CRASH_POSTGRES_ENV)?;
    let input = projection_crash_input_from_environment()?;
    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 6)?;
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let handler = projection_deployment_handler(&executor, workloads);
    match handler
        .execute(projection_deployment_command(&input)?, cqrs_context())
        .await?
    {
        Ok(_) => Err("Durable Cell projection crash probe returned before process death".into()),
        Err(error) => Err(format!(
            "Durable Cell projection crash probe failed before the blocked Workloads boundary: {error}"
        )
        .into()),
    }
}

fn projection_deployment_handler(
    executor: &PostgresExecutor,
    workloads: Arc<PostgresWorkloadRepository>,
) -> DeployDurableCellApplicationHandler {
    let applications = Arc::new(PostgresDurableCellApplicationRepository::new(
        executor.clone(),
    ));
    let workload_port = Arc::new(WorkloadsDurableCellWorkloadAdapter::new(
        applications.clone(),
        workloads.clone(),
        workloads.clone(),
    ));
    let node_pool_port = Arc::new(FleetDurableCellNodePoolAdapter::new(Arc::new(
        PostgresNodeRepository::new(executor.clone()),
    )));
    let secrets = Arc::new(PostgresSecretRepository::new(executor.clone()));
    let storage = Arc::new(DataDurableCellStorageAdapter::new(secrets.clone()));
    let secret_bindings = Arc::new(SecretsDurableCellBindingAdapter::new(
        exact_secret_version_access(secrets.clone()),
    ));
    DeployDurableCellApplicationHandler::new(
        applications,
        Arc::new(PostgresDurableCellDeploymentRepository::new(
            executor.clone(),
        )),
        workload_port,
        storage,
        secret_bindings,
        node_pool_port,
    )
}

fn projection_deployment_command(
    input: &ProjectionCrashInput,
) -> Result<DeployDurableCellApplication, Box<dyn std::error::Error>> {
    let profile = DurableCellServiceProfile::parse_acl(DURABLE_CELL_SERVICE_PROFILE_ACL)?;
    let storage_provider_profile =
        ObjectNamespaceProviderProfile::parse_acl(OBJECT_NAMESPACE_PROVIDER_PROFILE_ACL)?;
    let storage_credentials =
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id: input.environment_id,
            namespace_id: input.storage_namespace_id,
            generation: 1,
            provider_profile_digest: storage_provider_profile.digest().clone(),
            access_key_id: input.access_key_id,
            secret_access_key: input.secret_access_key,
            session_token: None,
        })?;
    let retention_policy =
        ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        })?;
    Ok(DeployDurableCellApplication {
        organization_id: input.organization_id,
        project_id: input.project_id,
        environment_id: input.environment_id,
        application_id: input.application_id,
        application_revision_id: input.application_revision_id,
        service_profile_acl: DURABLE_CELL_SERVICE_PROFILE_ACL.into(),
        storage_provider_profile_acl: Some(storage_provider_profile.canonical_acl().into()),
        workload_template: durable_cell_service_template(
            &profile,
            &storage_provider_profile,
            input.storage_namespace_id,
            input.access_key_id,
            input.secret_access_key,
        ),
        storage_credentials,
        retention_policy,
        node_pool_id: None,
        actor_principal_id: input.actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "durable-cell-c6-projection-recovery".into(),
        request_id: input.request_id,
    })
}

fn durable_cell_state_idempotency(
    record: &DurableCellApplicationRecord,
    expected_version: u64,
    key: &str,
) -> Result<IdempotencyRequest, Box<dyn std::error::Error>> {
    let application = &record.application;
    let canonical = serde_json::to_vec(&CanonicalDurableCellStateRequest {
        organization_id: application.organization_id,
        project_id: application.project_id,
        environment_id: application.environment_id,
        application_id: application.id,
        expected_version,
        desired_state: application.desired_state.as_str(),
    })?;
    Ok(IdempotencyRequest::new(
        format!(
            "organizations/{}/projects/{}/environments/{}/durable-cell-applications/{}/desired-state",
            application.organization_id,
            application.project_id,
            application.environment_id,
            application.id,
        ),
        key,
        &canonical,
    )?)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalDurableCellStateRequest<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    desired_state: &'a str,
}

fn durable_cell_service_template(
    profile: &DurableCellServiceProfile,
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    access_key_id: SecretVersionReference,
    secret_access_key: SecretVersionReference,
) -> ServiceTemplate {
    let publisher =
        DurableCellPublisherProfile::pinned_celld_v0_2_1().expect("pinned celld publisher");
    let provider_spec = provider_profile.spec();
    let provider_projection = DurableCellStorageProviderProfileProjection {
        digest: provider_profile.digest().clone(),
        endpoint: provider_spec.endpoint.clone(),
        region: provider_spec.region.clone(),
        bucket: provider_spec.bucket.clone(),
        prefix: provider_spec.prefix.clone(),
        virtual_hosted_style: provider_spec.virtual_hosted_style,
    };
    ServiceTemplate {
        artifact: OciArtifact {
            uri: publisher.image_uri().into(),
            digest: publisher.image_digest().to_string(),
            media_type: "application/vnd.oci.image.index.v1+json".into(),
        },
        process: compose_pinned_celld_service_process(
            &provider_projection,
            storage_namespace_id,
            8080,
            8081,
            &publisher,
        )
        .expect("pinned celld Service process"),
        secrets: vec![
            SecretBinding {
                name: "s0-access-key-id".into(),
                secret_id: access_key_id.secret_id,
                version: access_key_id.version,
                target: SecretBindingTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            },
            SecretBinding {
                name: "s0-secret-access-key".into(),
                secret_id: secret_access_key.secret_id,
                version: secret_access_key.version,
                target: SecretBindingTarget::Environment {
                    variable: "AWS_SECRET_ACCESS_KEY".into(),
                },
            },
        ],
        resources: ServiceResources {
            cpu_millis: 1000,
            memory_bytes: 512 * 1024 * 1024,
            pids: 256,
            ephemeral_storage_bytes: None,
        },
        ports: vec![
            ServicePort {
                name: profile.spec().public_runtime_port.clone(),
                container_port: 8080,
            },
            ServicePort {
                name: profile.spec().internal_runtime_port.clone(),
                container_port: 8081,
            },
        ],
        health: Some(HttpHealthCheck {
            port_name: profile.spec().public_runtime_port.clone(),
            path: profile.spec().health_path.clone(),
            interval_ms: 1000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5000,
        }),
    }
}

async fn create_postgres_secret(
    repository: &PostgresSecretRepository,
    tenant: DurableCellTenantFixture,
    name: &str,
    ciphertext: &str,
) -> Result<SecretVersionReference, Box<dyn std::error::Error>> {
    let secret_id = SecretId::new();
    let (secret, version) = Secret::create(
        secret_id,
        tenant.organization_id,
        tenant.project_id,
        tenant.environment_id,
        ResourceName::parse(name)?,
        EncryptedSecretValue::new("durable-cell-c6-key", ciphertext)?,
        tenant.created_at,
    )?;
    let request_id = Uuid::now_v7();
    let write = repository
        .create(CreateSecretWrite {
            event: SecretChanged::created(&secret, &version, request_id)?,
            idempotency: IdempotencyRequest::new(
                format!("durable-cell-c6-secret-{}", tenant.organization_id),
                secret_id.to_string(),
                secret_id.as_uuid().as_bytes(),
            )?,
            secret,
            version,
        })
        .await?;
    assert!(!write.replayed);
    Ok(SecretVersionReference::new(secret_id, 1)?)
}

async fn projection_boundary_evidence(
    database: &Database<PostgresDialect, PostgresExecutor>,
    input: &ProjectionCrashInput,
    projection: &DurableCellProjectionIdentity,
) -> Result<(i64, i64, i64, i64, i64), Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from durable_cell_deployments where organization_id = ",
            )
            .bind(input.organization_id.as_uuid())
            .append(" and application_id = ")
            .bind(input.application_id.as_uuid())
            .append(" and application_revision_id = ")
            .bind(input.application_revision_id.as_uuid())
            .append("), (select count(*) from workloads where organization_id = ")
            .bind(input.organization_id.as_uuid())
            .append(" and id = ")
            .bind(projection.workload_id.as_uuid())
            .append("), (select count(*) from workload_revisions where workload_id = ")
            .bind(projection.workload_id.as_uuid())
            .append(" and id = ")
            .bind(projection.workload_revision_id.as_uuid())
            .append("), (select count(*) from deployments where organization_id = ")
            .bind(input.organization_id.as_uuid())
            .append(" and id = ")
            .bind(projection.deployment_id.as_uuid())
            .append("), (select count(*) from operation_requests where organization_id = ")
            .bind(input.organization_id.as_uuid())
            .append(" and operation_id = ")
            .bind(projection.operation_id.as_uuid())
            .append(")"),
        )
        .await?)
}

async fn kill_projection_after_correlation(
    database: &Database<PostgresDialect, PostgresExecutor>,
    postgres_url: &str,
    input: &ProjectionCrashInput,
    projection: &DurableCellProjectionIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut probe = ProjectionCrashProcess::start(&std::env::current_exe()?, postgres_url, input)?;
    for _ in 0..600 {
        if let Some(status) = probe.try_wait()? {
            return Err(format!(
                "Durable Cell projection crash probe exited before the durable boundary with {status}"
            )
            .into());
        }
        let boundary = projection_boundary_evidence(database, input, projection).await?;
        if boundary.1 != 0 || boundary.2 != 0 || boundary.3 != 0 || boundary.4 != 0 {
            return Err(format!(
                "Workloads authority committed while its table lock was held: {boundary:?}"
            )
            .into());
        }
        if boundary.0 > 1 {
            return Err("Durable Cell crash probe duplicated its immutable correlation".into());
        }
        let blocked_workload_insert = database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from pg_stat_activity where datname = current_database() and pid <> pg_backend_pid() and wait_event_type = 'Lock' and lower(query) like '%insert%workloads%'",
                ),
            )
            .await?;
        if boundary.0 == 1 && blocked_workload_insert == 1 {
            let status = probe.kill_and_wait()?;
            require_projection_process_kill(status)?;
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from_millis(50)).await;
    }
    Err("Durable Cell projection crash probe did not reach the blocked Workloads boundary".into())
}

struct ProjectionCrashProcess {
    child: Option<Child>,
}

impl ProjectionCrashProcess {
    fn start(
        test_binary: &std::path::Path,
        postgres_url: &str,
        input: &ProjectionCrashInput,
    ) -> std::io::Result<Self> {
        let child = Command::new(test_binary)
            .arg(PROJECTION_CRASH_PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(PROJECTION_CRASH_PARENT_ENV, "1")
            .env(PROJECTION_CRASH_POSTGRES_ENV, postgres_url)
            .env(
                PROJECTION_CRASH_ORGANIZATION_ENV,
                input.organization_id.to_string(),
            )
            .env(PROJECTION_CRASH_PROJECT_ENV, input.project_id.to_string())
            .env(
                PROJECTION_CRASH_ENVIRONMENT_ENV,
                input.environment_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_APPLICATION_ENV,
                input.application_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_REVISION_ENV,
                input.application_revision_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_NAMESPACE_ENV,
                input.storage_namespace_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_ACCESS_KEY_ENV,
                input.access_key_id.secret_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_SECRET_KEY_ENV,
                input.secret_access_key.secret_id.to_string(),
            )
            .env(
                PROJECTION_CRASH_ACTOR_ENV,
                input.actor_principal_id.to_string(),
            )
            .env(PROJECTION_CRASH_REQUEST_ENV, input.request_id.to_string())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Durable Cell crash probe process disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("Durable Cell crash probe process disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for ProjectionCrashProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn require_projection_process_kill(status: ExitStatus) -> Result<(), Box<dyn std::error::Error>> {
    if status.success() {
        return Err("Durable Cell crash probe exited successfully instead of being killed".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(format!(
                "Durable Cell crash probe exited with {status} instead of SIGKILL"
            )
            .into());
        }
    }
    Ok(())
}

fn projection_crash_input_from_environment(
) -> Result<ProjectionCrashInput, Box<dyn std::error::Error>> {
    Ok(ProjectionCrashInput {
        organization_id: OrganizationId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_ORGANIZATION_ENV,
        )?),
        project_id: ProjectId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_PROJECT_ENV,
        )?),
        environment_id: EnvironmentId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_ENVIRONMENT_ENV,
        )?),
        application_id: DurableCellApplicationId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_APPLICATION_ENV,
        )?),
        application_revision_id: DurableCellApplicationRevisionId::from_uuid(
            required_projection_probe_uuid(PROJECTION_CRASH_REVISION_ENV)?,
        ),
        storage_namespace_id: StorageNamespaceId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_NAMESPACE_ENV,
        )?),
        access_key_id: SecretVersionReference::new(
            SecretId::from_uuid(required_projection_probe_uuid(
                PROJECTION_CRASH_ACCESS_KEY_ENV,
            )?),
            1,
        )?,
        secret_access_key: SecretVersionReference::new(
            SecretId::from_uuid(required_projection_probe_uuid(
                PROJECTION_CRASH_SECRET_KEY_ENV,
            )?),
            1,
        )?,
        actor_principal_id: PrincipalId::from_uuid(required_projection_probe_uuid(
            PROJECTION_CRASH_ACTOR_ENV,
        )?),
        request_id: required_projection_probe_uuid(PROJECTION_CRASH_REQUEST_ENV)?,
    })
}

fn required_projection_probe_uuid(name: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(Uuid::parse_str(&required_projection_probe_environment(
        name,
    )?)?)
}

fn required_projection_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("Durable Cell crash probe omitted {name}")))
}

async fn seed_durable_cell_tenant(
    database: &Database<PostgresDialect, PostgresExecutor>,
) -> Result<DurableCellTenantFixture, Box<dyn std::error::Error>> {
    let fixture = DurableCellTenantFixture {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        actor: PrincipalId::new(),
        // Keep successor fixture timestamps behind the wall clock so fast CI
        // execution cannot make a legitimate state request look regressive.
        created_at: Utc::now() - Duration::seconds(5),
    };
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(", 'Durable Cell tenant', ")
            .bind(format!("durable-cell-{}", fixture.organization_id))
            .append(", 1, ")
            .bind(fixture.created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(fixture.actor.as_uuid())
                .append(", 'human', 'Durable Cell owner', 1, ")
                .bind(fixture.created_at)
                .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(", ")
            .bind(fixture.project_id.as_uuid())
            .append(", 'Durable Cell project', 'durable-cell-project', 1, ")
            .bind(fixture.created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(fixture.organization_id.as_uuid())
                .append(", ")
                .bind(fixture.project_id.as_uuid())
                .append(", ")
                .bind(fixture.environment_id.as_uuid())
                .append(", 'Production', 'production', 1, ")
                .bind(fixture.created_at)
                .append(")"),
        )
        .await?;
    Ok(fixture)
}

async fn insert_typed_build_run(
    database: &Database<PostgresDialect, PostgresExecutor>,
    executor: &PostgresExecutor,
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

    let node_id = NodeId::new();
    let apply_command_id = NodeCommandId::new();
    let cleanup_command_id = NodeCommandId::new();
    let capabilities = NodeCapabilities::new(
        "durable-cell-build-runtime",
        "durable-cell-build-runtime-1",
        serde_json::json!({}),
    )?;
    database
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", ")
            .bind(format!("Durable Cell build {}", build.id))
            .append(", ")
            .bind(format!("durable-cell-build-{}", build.id))
            .append(", 'ready', ")
            .bind(Uuid::now_v7())
            .append(", 'test', 'durable-cell-build-runtime', 'durable-cell-build-runtime-1', ")
            .bind(capabilities.digest())
            .append(", ")
            .bind(capabilities.document().clone())
            .append(", ")
            .bind(build.requested_at)
            .append(", ")
            .bind(build.requested_at)
            .append(", 2, 1)"),
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
                .bind(build.requested_at)
                .append(", ")
                .bind(build.requested_at + Duration::minutes(1))
                .append(", ")
                .bind(build.id.as_uuid())
                .append(")"),
            )
            .await?;
    }

    let builds = PostgresBuildRunRepository::new(executor.clone());
    let mut build = builds.find(organization_id, build.id).await?;
    let mut at = build.updated_at;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_preparation(at)?;
    let mut build = builds.save(build, expected).await?;

    let input = super::build_runs_support::build_artifact('7', 1_024)?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_input(format!("sha256:{}", "2".repeat(64)), input.clone(), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.schedule(node_id, format!("sha256:{}", "3".repeat(64)), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.dispatch(apply_command_id, at)?;
    build = builds.save(build, expected).await?;

    let runtime_output = super::build_runs_support::build_artifact('8', 8_192)?;
    let box_output = super::build_runs_support::box_output(&runtime_output, &input)?;
    let descriptor = OciDescriptor::new(
        box_output.descriptor.media_type.clone(),
        box_output.descriptor.digest.clone(),
        box_output.descriptor.size,
    )?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_validation(box_output, at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_validated_output(
        ValidatedOciBuildOutput {
            artifact: runtime_output,
            descriptor: descriptor.clone(),
            platforms: vec![BuildPlatform::parse("linux/amd64")?],
            content_bytes: 2_048,
            blob_count: 3,
        },
        at,
    )?;
    build = builds.save(build, expected).await?;

    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s/durable-cell-builds/{}", build.id),
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

    let bundle_digest = format!("sha256:{}", marker_text.repeat(64));
    let bundle = BuildArtifact::new(
        artifact_uri(&bundle_digest)?,
        bundle_digest,
        DURABLE_CELL_BUNDLE_MEDIA_TYPE,
        1_024,
    )?;
    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.record_published_output(bundle.clone(), at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_attestation(at)?;
    build = builds.save(build, expected).await?;

    at += Duration::milliseconds(1);
    let evidence = crate::build_evidence_support::evidence_for(
        &build,
        at,
        &format!("https://github.com/a3s-lab/cell-fixture-{marker}"),
        &marker_text.repeat(40),
        None,
        false,
    )?;
    let expected = build.aggregate_version;
    build.record_evidence(evidence, at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.begin_cleanup(cleanup_command_id, at)?;
    build = builds.save(build, expected).await?;

    let expected = build.aggregate_version;
    at += Duration::milliseconds(1);
    build.complete(at)?;
    let build = builds.finalize(build, expected).await?;
    if build.published_output.as_ref() != Some(&bundle) {
        return Err("Durable Cell BuildRun lost its typed bundle output".into());
    }
    Ok(build.id)
}

fn definition(
    build_run_id: BuildRunId,
    marker: char,
    write_version: u64,
) -> Result<DurableCellApplicationDefinition, String> {
    definition_with_service_profile(build_run_id, marker, write_version, digest('f'))
}

fn definition_with_service_profile(
    build_run_id: BuildRunId,
    marker: char,
    write_version: u64,
    service_profile_digest: Sha256Digest,
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
        service_profile_digest,
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
