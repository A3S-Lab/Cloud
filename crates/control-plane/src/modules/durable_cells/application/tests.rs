use super::*;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
    DurableCellApplicationDesiredState, DurableCellClassSpec, DurableCellRollbackPolicy,
    DurableCellStateSchema,
};
use crate::modules::durable_cells::infrastructure::InMemoryDurableCellApplicationRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::events::EnvironmentCreated;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest, SourceRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn cqrs_authorizes_before_replay_and_preserves_exact_state_history() {
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor_principal_id = PrincipalId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let environment = Environment::create(
        organization_id,
        project_id,
        environment_id,
        EnvironmentName::parse("Production").expect("environment name"),
        now,
    );
    IEnvironmentRepository::create(
        projects.as_ref(),
        environment.clone(),
        EnvironmentCreated::envelope(&environment, Uuid::now_v7()).expect("environment event"),
        IdempotencyRequest::new(
            "durable-cell-application-environment",
            "create",
            environment_id.as_uuid().as_bytes(),
        )
        .expect("environment idempotency"),
    )
    .await
    .expect("store environment");

    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let initial_build_run_id = reserve_build(
        builds.as_ref(),
        organization_id,
        project_id,
        environment_id,
        now,
    )
    .await;
    let successor_build_run_id = reserve_build(
        builds.as_ref(),
        organization_id,
        project_id,
        environment_id,
        now + Duration::milliseconds(1),
    )
    .await;
    let foreign_build_run_id = reserve_build(
        builds.as_ref(),
        organization_id,
        project_id,
        EnvironmentId::new(),
        now + Duration::milliseconds(2),
    )
    .await;

    let applications = Arc::new(InMemoryDurableCellApplicationRepository::new());
    let create_handler =
        CreateDurableCellApplicationHandler::new(projects, applications.clone(), builds.clone());
    let create = CreateDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        name: "Tenant counters".into(),
        definition_acl: definition(initial_build_run_id, 'a', 1),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "create-tenant-counters".into(),
        request_id: Uuid::now_v7(),
    };
    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create Durable Cell application");
    assert!(!created.replayed);

    let denied_replay = create_handler
        .execute(
            CreateDurableCellApplication {
                resource_access: denied_access(project_id),
                ..create.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(denied_replay, Err(ApplicationError::NotFound(_))));

    let replay = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, created.record);

    let conflicting = create_handler
        .execute(
            CreateDurableCellApplication {
                definition_acl: definition(initial_build_run_id, '9', 1),
                ..create.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(conflicting, Err(ApplicationError::Conflict(_))));

    let foreign_build = create_handler
        .execute(
            CreateDurableCellApplication {
                name: "Foreign build".into(),
                definition_acl: definition(foreign_build_run_id, 'c', 1),
                idempotency_key: "foreign-build".into(),
                request_id: Uuid::now_v7(),
                ..create
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(foreign_build, Err(ApplicationError::NotFound(_))));

    let revise_handler =
        ReviseDurableCellApplicationHandler::new(applications.clone(), builds.clone());
    let revise = ReviseDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        application_id: created.record.application.id,
        expected_version: 1,
        definition_acl: definition(successor_build_run_id, 'b', 2),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "revise-tenant-counters".into(),
        request_id: Uuid::now_v7(),
    };
    let revised = revise_handler
        .execute(revise.clone(), context())
        .await
        .expect("command framework")
        .expect("revise Durable Cell application");
    assert!(!revised.replayed);
    assert_eq!(revised.record.application.aggregate_version, 2);
    assert!(
        revise_handler
            .execute(revise, context())
            .await
            .expect("command framework")
            .expect("revision replay")
            .replayed
    );

    let stop_handler = StopDurableCellApplicationHandler::new(applications.clone());
    let stop = StopDurableCellApplication {
        organization_id,
        project_id,
        environment_id,
        application_id: created.record.application.id,
        expected_version: 2,
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "stop-tenant-counters".into(),
        request_id: Uuid::now_v7(),
    };
    let stopped = stop_handler
        .execute(stop.clone(), context())
        .await
        .expect("command framework")
        .expect("stop Durable Cell application");
    assert_eq!(
        stopped.record.application.desired_state,
        DurableCellApplicationDesiredState::Stopped
    );
    assert_eq!(stopped.record.application.aggregate_version, 3);

    let denied_stop_replay = stop_handler
        .execute(
            StopDurableCellApplication {
                resource_access: denied_access(project_id),
                ..stop.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(
        denied_stop_replay,
        Err(ApplicationError::NotFound(_))
    ));

    let no_op_stop = stop_handler
        .execute(
            StopDurableCellApplication {
                expected_version: 3,
                idempotency_key: "stop-again".into(),
                request_id: Uuid::now_v7(),
                ..stop.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(no_op_stop, Err(ApplicationError::Conflict(_))));

    let start_handler = StartDurableCellApplicationHandler::new(applications.clone());
    let started = start_handler
        .execute(
            StartDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: created.record.application.id,
                expected_version: 3,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "start-tenant-counters".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("start Durable Cell application");
    assert_eq!(
        started.record.application.desired_state,
        DurableCellApplicationDesiredState::Running
    );
    assert_eq!(started.record.application.aggregate_version, 4);

    let historical_stop = stop_handler
        .execute(stop, context())
        .await
        .expect("command framework")
        .expect("historical stop replay");
    assert!(historical_stop.replayed);
    assert_eq!(historical_stop.record, stopped.record);

    let current = GetDurableCellApplicationHandler::new(applications.clone())
        .execute(
            GetDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: created.record.application.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get Durable Cell application");
    assert_eq!(current, started.record);

    let hidden = GetDurableCellApplicationHandler::new(applications.clone())
        .execute(
            GetDurableCellApplication {
                organization_id,
                project_id,
                environment_id,
                application_id: created.record.application.id,
                resource_access: denied_access(project_id),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(hidden, Err(ApplicationError::NotFound(_))));

    let listed = ListDurableCellApplicationsHandler::new(applications.clone())
        .execute(
            ListDurableCellApplications {
                organization_id,
                project_id,
                environment_id,
                limit: DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list Durable Cell applications");
    assert_eq!(listed, vec![started.record.application.clone()]);

    let unbounded = ListDurableCellApplicationsHandler::new(applications.clone())
        .execute(
            ListDurableCellApplications {
                organization_id,
                project_id,
                environment_id,
                limit: MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT + 1,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(unbounded, Err(ApplicationError::Invalid(_))));

    let history = ListDurableCellApplicationRevisionsHandler::new(applications.clone())
        .execute(
            ListDurableCellApplicationRevisions {
                organization_id,
                project_id,
                environment_id,
                application_id: created.record.application.id,
                limit: 50,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list Durable Cell revisions");
    assert_eq!(
        history,
        vec![
            revised.record.revision.clone(),
            created.record.revision.clone()
        ]
    );

    let initial = GetDurableCellApplicationRevisionHandler::new(applications.clone())
        .execute(
            GetDurableCellApplicationRevision {
                organization_id,
                project_id,
                environment_id,
                application_id: created.record.application.id,
                revision_id: created.record.revision.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get initial Durable Cell revision");
    assert_eq!(initial, created.record.revision);

    let events = applications.outbox_events().await;
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].event_key, "durable-cell.application.created");
    assert_eq!(events[1].event_key, "durable-cell.application.revised");
    assert_eq!(
        events[2].event_key,
        "durable-cell.application.state-requested"
    );
    assert_eq!(
        events[3].event_key,
        "durable-cell.application.state-requested"
    );
}

async fn reserve_build(
    builds: &InMemoryBuildRunRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    accepted_at: chrono::DateTime<Utc>,
) -> BuildRunId {
    builds
        .add_source_revision(
            organization_id,
            project_id,
            environment_id,
            SourceRevisionId::new(),
            accepted_at,
        )
        .await;
    builds
        .reserve_pending(1, accepted_at)
        .await
        .expect("reserve BuildRun")
        .into_iter()
        .next()
        .expect("reserved BuildRun")
        .id
}

fn definition(build_run_id: BuildRunId, marker: char, write_version: u64) -> String {
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
    .expect("Durable Cell definition")
    .canonical_acl()
    .to_owned()
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn denied_access(project_id: ProjectId) -> ResourceAccessEvaluator {
    ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
        project_id,
        environment_id: EnvironmentId::new(),
    }])
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
