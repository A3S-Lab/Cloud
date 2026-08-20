use super::applications_support::{cqrs_contract, digest, seed_scope};
use super::workflow_semantic_contracts_support::semantic_revision;
use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_control_plane::modules::applications::{
    Application, ApplicationEndUser, ApplicationInvocation, ApplicationInvocationStatus,
    ApplicationMessage, ApplicationRecord, ApplicationRelease, ApplicationReleasePublished,
    ApplicationResponseMode, ApplicationSession, ApplicationWorkflowRunRequest,
    ComposeApplicationInvocationWorkflowRun, ComposeApplicationInvocationWorkflowRunHandler,
    ConversationVariableRevision, CreateApplicationWrite, IApplicationRepository,
    IApplicationSessionRepository, IApplicationWorkflowRevisionPort, IApplicationWorkflowRunPort,
    OpenApplicationSessionWrite, PostgresApplicationRepository,
    PostgresApplicationSessionRepository, RequestApplicationInvocationWrite,
    WorkflowApplicationReleaseEvidenceReader, WorkflowApplicationRunService,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, IdempotencyRequest, OntologyId, OntologyRevisionId, OrganizationId,
    PrincipalId, ProjectId, ResourceName, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_cloud_control_plane::modules::workflow::domain::{
    CreateOntologyWrite, OntologyRecord, OntologyRevisionPublished,
};
use a3s_cloud_control_plane::modules::workflow::{
    CreateWorkflowDefinitionWrite, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowRunRepository, Ontology, OntologyContract, OntologyName, OntologyObjectType,
    OntologyRevision, OntologySpec, PostgresOntologyRepository,
    PostgresWorkflowDefinitionRepository, PostgresWorkflowGoalRepository,
    PostgresWorkflowRunRepository, WorkflowDefinition, WorkflowDefinitionRecord,
    WorkflowRevisionPublished,
};
use a3s_orm::{Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;

pub(super) async fn exercise_application_workflow_run_composition(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    seed_scope(&database, organization_id, project_id, actor, created_at).await?;

    let workflow_definition_id = WorkflowDefinitionId::new();
    let workflow_revision_id = WorkflowRevisionId::new();
    let workflow_revision = semantic_revision(
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id,
        actor,
        created_at,
        false,
    );
    persist_workflow(&executor, &workflow_revision).await?;
    let workflow_reader = WorkflowApplicationReleaseEvidenceReader::new(Arc::new(
        PostgresWorkflowDefinitionRepository::new(executor.clone()),
    ));
    let workflow_evidence = workflow_reader
        .resolve_revision(
            organization_id,
            project_id,
            workflow_definition_id,
            workflow_revision_id,
        )
        .await?;

    let application_release = ApplicationRelease::initial(
        organization_id,
        project_id,
        ApplicationId::new(),
        ApplicationReleaseId::new(),
        cqrs_contract(&workflow_evidence, '7'),
        actor,
        created_at + Duration::seconds(1),
    )?;
    let application = Application::create(
        application_release.application_id,
        ResourceName::parse("Composed workflow application")?,
        "Exact Application invocation to WorkflowRun composition".into(),
        &application_release,
    )?;
    let application_request_id = Uuid::now_v7();
    PostgresApplicationRepository::new(executor.clone())
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(
                &application,
                &application_release,
                application_request_id,
            )?,
            actor_principal_id: actor,
            request_id: application_request_id,
            idempotency: IdempotencyRequest::new(
                "application-workflow-run-composition",
                "application",
                application_release.contract.canonical_acl().as_bytes(),
            )?,
            record: ApplicationRecord::new(application.clone(), application_release.clone())?,
        })
        .await?;

    let ontology_revision =
        persist_ontology(&executor, organization_id, project_id, actor, created_at).await?;
    let sessions = PostgresApplicationSessionRepository::new(executor.clone());
    let opened_at = created_at + Duration::seconds(2);
    let end_user = ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &application_release,
        Some(actor),
        actor,
        opened_at,
    )?;
    let session_id = ApplicationSessionId::new();
    let variables = ConversationVariableRevision::initial(
        session_id,
        &application_release,
        json!({"locale": "en-US"}),
        opened_at,
    )?;
    let session = ApplicationSession::create(
        session_id,
        &application_release,
        &end_user,
        &variables,
        opened_at,
    )?;
    sessions
        .open_session(OpenApplicationSessionWrite {
            release: application_release.clone(),
            end_user,
            session: session.clone(),
            initial_variables: variables,
        })
        .await?;
    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &application_release,
        ApplicationResponseMode::Streaming,
        json!({"ticketId": "T-42"}),
        created_at + Duration::seconds(3),
    )?;
    sessions
        .request_invocation(RequestApplicationInvocationWrite {
            input_message: ApplicationMessage::input(
                &session,
                &invocation,
                invocation.requested_at,
            )?,
            invocation: invocation.clone(),
            expected_session_version: session.aggregate_version,
        })
        .await?;

    let command = ComposeApplicationInvocationWorkflowRun {
        organization_id,
        project_id,
        application_id: application.id,
        session_id,
        invocation_id: invocation.id,
        ontology_id: ontology_revision.ontology_id,
        ontology_revision_id: ontology_revision.id,
        ontology_digest: ontology_revision.contract.digest().clone(),
        environment_id: None,
        requested_by: actor,
        timeout_seconds: 120,
    };
    let first = composition_handler(&executor)
        .execute(command.clone(), cqrs_context())
        .await?
        .map_err(|error| format!("compose Application WorkflowRun: {error}"))?;
    assert!(!first.replayed);
    assert_eq!(
        first.invocation.status,
        ApplicationInvocationStatus::Running
    );
    assert_eq!(
        first.invocation.workflow_run_id,
        Some(first.workflow.workflow_run_id)
    );

    let restarted = composition_handler(&executor);
    let replay = restarted
        .execute(command.clone(), cqrs_context())
        .await?
        .map_err(|error| format!("adopt Application WorkflowRun after restart: {error}"))?;
    assert!(replay.replayed);
    assert_eq!(replay.invocation, first.invocation);
    assert_eq!(replay.workflow, first.workflow);

    let timeout_drift = restarted
        .execute(
            ComposeApplicationInvocationWorkflowRun {
                timeout_seconds: 121,
                ..command.clone()
            },
            cqrs_context(),
        )
        .await?;
    assert!(matches!(timeout_drift, Err(ApplicationError::Conflict(_))));
    let ontology_drift = restarted
        .execute(
            ComposeApplicationInvocationWorkflowRun {
                ontology_digest: digest('8'),
                ..command
            },
            cqrs_context(),
        )
        .await?;
    assert!(matches!(ontology_drift, Err(ApplicationError::Conflict(_))));

    let runs = PostgresWorkflowRunRepository::new(executor.clone());
    let persisted_run = runs
        .find(organization_id, first.workflow.workflow_run_id)
        .await?
        .expect("composed WorkflowRun");
    assert_eq!(persisted_run.run.id, first.workflow.workflow_run_id);
    assert_eq!(persisted_run.run.plan_digest, first.workflow.plan_digest);

    let cancellation_request = ApplicationWorkflowRunRequest::from_invocation(
        &application_release,
        &session,
        &first.invocation,
        ontology_revision.ontology_id,
        ontology_revision.id,
        ontology_revision.contract.digest().clone(),
        None,
        actor,
        120,
    )?;
    let workflow_runs = workflow_run_service(&executor);
    let cancelled = workflow_runs
        .request_cancellation(
            &cancellation_request,
            "Application invocation cancellation race",
            created_at + Duration::seconds(4),
        )
        .await?
        .expect("cancelled WorkflowRun evidence");
    assert_eq!(cancelled, first.workflow);
    assert_eq!(
        workflow_runs
            .request_cancellation(
                &cancellation_request,
                "Application invocation cancellation race",
                created_at + Duration::seconds(4),
            )
            .await?
            .expect("replayed WorkflowRun cancellation"),
        cancelled
    );
    assert_eq!(
        runs.find(organization_id, first.workflow.workflow_run_id)
            .await?
            .expect("cancelled WorkflowRun")
            .run
            .status,
        a3s_cloud_control_plane::modules::workflow::WorkflowRunStatus::Cancelling
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64, i64)>(
                    "select (select count(*) from workflow_goals where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from workflow_plan_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from workflow_runs where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(")"),
            )
            .await?,
        (1, 1, 1)
    );
    Ok(())
}

fn composition_handler(
    executor: &PostgresExecutor,
) -> ComposeApplicationInvocationWorkflowRunHandler {
    ComposeApplicationInvocationWorkflowRunHandler::new(
        Arc::new(PostgresApplicationRepository::new(executor.clone())),
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
        Arc::new(workflow_run_service(executor)),
    )
}

fn workflow_run_service(executor: &PostgresExecutor) -> WorkflowApplicationRunService {
    WorkflowApplicationRunService::new(
        Arc::new(PostgresWorkflowDefinitionRepository::new(executor.clone())),
        Arc::new(PostgresOntologyRepository::new(executor.clone())),
        Arc::new(PostgresWorkflowGoalRepository::new(executor.clone())),
        Arc::new(PostgresWorkflowRunRepository::new(executor.clone())),
    )
}

async fn persist_workflow(
    executor: &PostgresExecutor,
    revision: &a3s_cloud_control_plane::modules::workflow::WorkflowRevision,
) -> Result<(), Box<dyn std::error::Error>> {
    let definition = WorkflowDefinition::create(
        revision.organization_id,
        revision.project_id,
        revision.workflow_definition_id,
        revision.contract.spec().name.clone(),
        revision.contract.spec().description.clone(),
        revision.id,
        revision.contract.digest().clone(),
        revision.created_by,
        revision.created_at,
    )?;
    let request_id = Uuid::now_v7();
    PostgresWorkflowDefinitionRepository::new(executor.clone())
        .create(CreateWorkflowDefinitionWrite {
            event: WorkflowRevisionPublished::created(&definition, revision, request_id)?,
            record: WorkflowDefinitionRecord {
                definition,
                revision: revision.clone(),
            },
            actor_principal_id: revision.created_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-workflow-run-composition",
                "workflow",
                revision.contract.canonical_acl().as_bytes(),
            )?,
        })
        .await?;
    Ok(())
}

async fn persist_ontology(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<OntologyRevision, Box<dyn std::error::Error>> {
    let ontology_id = OntologyId::new();
    let revision = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        OntologyRevisionId::new(),
        OntologyContract::from_spec(OntologySpec {
            name: "Application invocation ontology".into(),
            description: "Exact composition test authority".into(),
            object_types: vec![OntologyObjectType {
                id: "ticket".into(),
                label: "Ticket".into(),
                schema_digest: digest('9'),
                key_fields: vec!["ticketId".into()],
            }],
            relation_types: Vec::new(),
            rules: Vec::new(),
        })?,
        actor,
        created_at,
    );
    let ontology = Ontology::create(
        organization_id,
        project_id,
        ontology_id,
        OntologyName::parse(revision.contract.spec().name.clone())?,
        revision.contract.spec().description.clone(),
        revision.id,
        revision.contract.digest().clone(),
        actor,
        created_at,
    )?;
    let request_id = Uuid::now_v7();
    PostgresOntologyRepository::new(executor.clone())
        .create(CreateOntologyWrite {
            event: OntologyRevisionPublished::created(&ontology, &revision, request_id)?,
            record: OntologyRecord {
                ontology,
                revision: revision.clone(),
            },
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-workflow-run-composition",
                "ontology",
                revision.contract.canonical_acl().as_bytes(),
            )?,
        })
        .await?;
    Ok(revision)
}

fn cqrs_context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
