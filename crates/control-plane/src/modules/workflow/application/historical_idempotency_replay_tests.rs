use super::commands::create_workflow_goal::{CreateWorkflowGoal, CreateWorkflowGoalHandler};
use super::commands::revise_workflow_definition::{
    ReviseWorkflowDefinition, ReviseWorkflowDefinitionHandler,
};
use super::commands::start_workflow_run::{StartWorkflowRun, StartWorkflowRunHandler};
use super::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionPublicationProvenance,
    WorkflowDefinitionPublicationRequest, WorkflowDefinitionPublicationService, WorkflowPayloadAcl,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::events::ProjectCreated;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::projects::domain::value_objects::ProjectName;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRevisionId};
use crate::modules::workflow::domain::{
    CreateOntologyWrite, CreateWorkflowDefinitionWrite, CreateWorkflowGoalWrite,
    CreateWorkflowRunWrite, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, OntologyRevisionPublished,
    ReviseWorkflowDefinitionWrite, WorkflowContract, WorkflowDefinitionRecord,
    WorkflowGoalCompiled, WorkflowGoalContract, WorkflowPlanCompiler, WorkflowRevision,
    WorkflowRevisionPublished, WorkflowRunRequested,
};
use crate::modules::workflow::test_support::{
    historic_provider_workflow_fixture, HistoricProviderWorkflowFixture,
};
use crate::modules::workflow::{
    InMemoryOntologyRepository, InMemoryWorkflowDefinitionRepository,
    InMemoryWorkflowGoalRepository, InMemoryWorkflowRunRepository,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use chrono::Duration;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const CREATE_KEY: &str = "historic-definition";
const REVISE_KEY: &str = "historic-revision";
const GOAL_KEY: &str = "historic-goal";
const RUN_KEY: &str = "historic-run";

#[tokio::test]
async fn publication_replays_historic_unsupported_revision_before_runtime_admission() {
    let fixture = historic_provider_workflow_fixture();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    seed_project(projects.as_ref(), &fixture).await;

    let scope = definition_scope(&fixture);
    let idempotency = publication_idempotency(&fixture, &scope, CREATE_KEY);
    seed_definition(workflows.as_ref(), &fixture, idempotency).await;
    let service = WorkflowDefinitionPublicationService::new(projects, workflows);

    let replay = service
        .publish(publication_request(
            &fixture,
            CREATE_KEY,
            fixture.revision.contract.canonical_acl().to_owned(),
        ))
        .await
        .expect("historic publication replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, fixture.definition_record());

    let conflict = service
        .publish(publication_request(
            &fixture,
            CREATE_KEY,
            workflow_acl_with_name(&fixture, "Drifted historic provider workflow"),
        ))
        .await
        .expect_err("same key with different publication input must conflict");
    assert_idempotency_conflict(conflict);

    let rejected = service
        .publish(publication_request(
            &fixture,
            "new-definition",
            fixture.revision.contract.canonical_acl().to_owned(),
        ))
        .await
        .expect_err("new unsupported publication must remain rejected");
    assert_runtime_admission_error(rejected);
}

#[tokio::test]
async fn revision_replays_historic_unsupported_successor_before_runtime_admission() {
    let fixture = historic_provider_workflow_fixture();
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    seed_definition(
        workflows.as_ref(),
        &fixture,
        IdempotencyRequest::new("test/workflow-definitions", "parent", b"parent")
            .expect("parent idempotency"),
    )
    .await;
    let successor = successor_record(&fixture, "Historic provider workflow revision");
    let idempotency = revision_idempotency(&fixture, &successor, REVISE_KEY);
    let event = WorkflowRevisionPublished::revised(
        &successor.definition,
        &successor.revision,
        Uuid::now_v7(),
    )
    .expect("revision event");
    IWorkflowDefinitionRepository::revise(
        workflows.as_ref(),
        ReviseWorkflowDefinitionWrite {
            record: successor.clone(),
            expected_version: 1,
            event,
            actor_principal_id: fixture.principal_id,
            request_id: Uuid::now_v7(),
            idempotency,
        },
    )
    .await
    .expect("seed historic successor");
    let handler = ReviseWorkflowDefinitionHandler::new(workflows);

    let replay = handler
        .execute(
            revision_command(&fixture, &successor, REVISE_KEY),
            context(),
        )
        .await
        .expect("command execution")
        .expect("historic revision replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, successor);

    let mut drift = successor.clone();
    let drift_contract = contract_with_name(&fixture, "Drifted historic provider revision");
    drift.revision = WorkflowRevision::successor(
        &fixture.revision,
        WorkflowRevisionId::new(),
        drift_contract.clone(),
        fixture.revision.payloads.clone(),
        fixture.principal_id,
        fixture.revision.created_at + Duration::seconds(2),
    )
    .expect("drift revision");
    drift.definition = fixture
        .definition
        .advance(
            1,
            drift_contract.spec().name.clone(),
            drift_contract.spec().description.clone(),
            drift.revision.id,
            drift_contract.digest().clone(),
            drift.revision.created_at,
        )
        .expect("drift definition");
    let conflict = handler
        .execute(revision_command(&fixture, &drift, REVISE_KEY), context())
        .await
        .expect("command execution")
        .expect_err("same key with different revision input must conflict");
    assert_idempotency_conflict(conflict);

    let rejected = handler
        .execute(
            revision_command(&fixture, &successor, "new-revision"),
            context(),
        )
        .await
        .expect("command execution")
        .expect_err("new unsupported revision must remain rejected");
    assert_runtime_admission_error(rejected);
}

#[tokio::test]
async fn goal_replays_historic_plan_before_runtime_admission() {
    let fixture = historic_provider_workflow_fixture();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let ontologies = Arc::new(InMemoryOntologyRepository::new());
    let goals = Arc::new(InMemoryWorkflowGoalRepository::new());
    seed_project(projects.as_ref(), &fixture).await;
    seed_definition(
        workflows.as_ref(),
        &fixture,
        IdempotencyRequest::new("test/workflow-definitions", "goal-authority", b"workflow")
            .expect("workflow idempotency"),
    )
    .await;
    seed_ontology(ontologies.as_ref(), &fixture).await;
    seed_goal(
        goals.as_ref(),
        &fixture,
        goal_idempotency(&fixture, GOAL_KEY, &fixture.goal.contract),
    )
    .await;
    let handler =
        CreateWorkflowGoalHandler::new(projects.clone(), projects, workflows, ontologies, goals);

    let replay = handler
        .execute(
            goal_command(&fixture, GOAL_KEY, &fixture.goal.contract),
            context(),
        )
        .await
        .expect("command execution")
        .expect("historic Goal replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, fixture.goal_record());

    let drift_contract = goal_contract_with_input(&fixture, json!({"drift": true}));
    let conflict = handler
        .execute(goal_command(&fixture, GOAL_KEY, &drift_contract), context())
        .await
        .expect("command execution")
        .expect_err("same key with different Goal input must conflict");
    assert_idempotency_conflict(conflict);

    let rejected = handler
        .execute(
            goal_command(&fixture, "new-goal", &fixture.goal.contract),
            context(),
        )
        .await
        .expect("command execution")
        .expect_err("new Plan compilation must remain rejected");
    assert_runtime_admission_error(rejected);
}

#[tokio::test]
async fn run_replays_historic_execution_before_runtime_admission() {
    let fixture = historic_provider_workflow_fixture();
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let goals = Arc::new(InMemoryWorkflowGoalRepository::new());
    let runs = Arc::new(InMemoryWorkflowRunRepository::new());
    seed_definition(
        workflows.as_ref(),
        &fixture,
        IdempotencyRequest::new("test/workflow-definitions", "run-authority", b"workflow")
            .expect("workflow idempotency"),
    )
    .await;
    seed_goal(
        goals.as_ref(),
        &fixture,
        IdempotencyRequest::new("test/workflow-goals", "run-goal", b"goal")
            .expect("goal idempotency"),
    )
    .await;
    seed_run(
        runs.as_ref(),
        &fixture,
        run_idempotency(&fixture, RUN_KEY, 3_600),
    )
    .await;
    let handler = StartWorkflowRunHandler::new(goals, workflows, runs);

    let replay = handler
        .execute(run_command(&fixture, RUN_KEY, 3_600), context())
        .await
        .expect("command execution")
        .expect("historic Run replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, fixture.run_record);

    let conflict = handler
        .execute(run_command(&fixture, RUN_KEY, 3_601), context())
        .await
        .expect("command execution")
        .expect_err("same key with different Run input must conflict");
    assert_idempotency_conflict(conflict);

    let rejected = handler
        .execute(run_command(&fixture, "new-run", 3_600), context())
        .await
        .expect("command execution")
        .expect_err("new Run compilation must remain rejected");
    assert_runtime_admission_error(rejected);
}

async fn seed_project(
    projects: &InMemoryProjectsRepository,
    fixture: &HistoricProviderWorkflowFixture,
) {
    let project = Project::create(
        fixture.organization_id,
        fixture.project_id,
        ProjectName::parse("historic-provider").expect("project name"),
        fixture.revision.created_at,
    );
    let event = ProjectCreated::envelope(&project, Uuid::now_v7()).expect("project event");
    IProjectRepository::create(
        projects,
        project,
        event,
        IdempotencyRequest::new("test/projects", "historic-provider", b"project")
            .expect("project idempotency"),
    )
    .await
    .expect("seed project");
}

async fn seed_definition(
    workflows: &InMemoryWorkflowDefinitionRepository,
    fixture: &HistoricProviderWorkflowFixture,
    idempotency: IdempotencyRequest,
) {
    let event =
        WorkflowRevisionPublished::created(&fixture.definition, &fixture.revision, Uuid::now_v7())
            .expect("definition event");
    IWorkflowDefinitionRepository::create(
        workflows,
        CreateWorkflowDefinitionWrite {
            record: fixture.definition_record(),
            event,
            actor_principal_id: fixture.principal_id,
            request_id: Uuid::now_v7(),
            idempotency,
        },
    )
    .await
    .expect("seed definition");
}

async fn seed_ontology(
    ontologies: &InMemoryOntologyRepository,
    fixture: &HistoricProviderWorkflowFixture,
) {
    let event = OntologyRevisionPublished::created(
        &fixture.ontology,
        &fixture.ontology_revision,
        Uuid::now_v7(),
    )
    .expect("ontology event");
    IOntologyRepository::create(
        ontologies,
        CreateOntologyWrite {
            record: fixture.ontology_record(),
            event,
            actor_principal_id: fixture.principal_id,
            request_id: Uuid::now_v7(),
            idempotency: IdempotencyRequest::new(
                "test/ontologies",
                "historic-provider",
                b"ontology",
            )
            .expect("ontology idempotency"),
        },
    )
    .await
    .expect("seed ontology");
}

async fn seed_goal(
    goals: &InMemoryWorkflowGoalRepository,
    fixture: &HistoricProviderWorkflowFixture,
    idempotency: IdempotencyRequest,
) {
    let event =
        WorkflowGoalCompiled::envelope(&fixture.goal, &fixture.plan_revision, Uuid::now_v7())
            .expect("Goal event");
    IWorkflowGoalRepository::create(
        goals,
        CreateWorkflowGoalWrite {
            record: fixture.goal_record(),
            event,
            actor_principal_id: fixture.principal_id,
            request_id: Uuid::now_v7(),
            idempotency,
        },
    )
    .await
    .expect("seed Goal");
}

async fn seed_run(
    runs: &InMemoryWorkflowRunRepository,
    fixture: &HistoricProviderWorkflowFixture,
    idempotency: IdempotencyRequest,
) {
    let event =
        WorkflowRunRequested::envelope(&fixture.run_record.run, Uuid::now_v7()).expect("Run event");
    IWorkflowRunRepository::create(
        runs,
        CreateWorkflowRunWrite {
            record: fixture.run_record.clone(),
            event,
            actor_principal_id: fixture.principal_id,
            request_id: Uuid::now_v7(),
            idempotency,
        },
    )
    .await
    .expect("seed Run");
}

fn publication_request(
    fixture: &HistoricProviderWorkflowFixture,
    key: &str,
    definition_acl: String,
) -> WorkflowDefinitionPublicationRequest {
    WorkflowDefinitionPublicationRequest {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        definition_id: crate::modules::shared_kernel::domain::WorkflowDefinitionId::new(),
        revision_id: WorkflowRevisionId::new(),
        definition_acl,
        payloads: payload_acls(fixture),
        semantic_contracts: None,
        provenance: WorkflowDefinitionPublicationProvenance::UserAuthored,
        actor_principal_id: fixture.principal_id,
        idempotency_scope: definition_scope(fixture),
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
    }
}

fn revision_command(
    fixture: &HistoricProviderWorkflowFixture,
    record: &WorkflowDefinitionRecord,
    key: &str,
) -> ReviseWorkflowDefinition {
    ReviseWorkflowDefinition {
        organization_id: fixture.organization_id,
        workflow_definition_id: fixture.definition.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        expected_version: 1,
        definition_acl: record.revision.contract.canonical_acl().to_owned(),
        payloads: payload_acls(fixture),
        semantic_contracts: None,
        actor_principal_id: fixture.principal_id,
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
    }
}

fn goal_command(
    fixture: &HistoricProviderWorkflowFixture,
    key: &str,
    contract: &WorkflowGoalContract,
) -> CreateWorkflowGoal {
    CreateWorkflowGoal {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        goal_acl: contract.canonical_acl().to_owned(),
        actor_principal_id: fixture.principal_id,
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
    }
}

fn run_command(
    fixture: &HistoricProviderWorkflowFixture,
    key: &str,
    timeout_seconds: u64,
) -> StartWorkflowRun {
    StartWorkflowRun {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        workflow_goal_id: fixture.goal.id,
        plan_revision_id: fixture.plan_revision.id,
        timeout_seconds: Some(timeout_seconds),
        actor_principal_id: fixture.principal_id,
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
        requested_at: fixture.revision.created_at + Duration::seconds(30),
    }
}

fn publication_idempotency(
    fixture: &HistoricProviderWorkflowFixture,
    scope: &str,
    key: &str,
) -> IdempotencyRequest {
    let canonical = serde_json::to_vec(&json!({
        "organizationId": fixture.organization_id,
        "projectId": fixture.project_id,
        "contentDigest": fixture.revision.contract.digest(),
        "payloadSetDigest": fixture.revision.payload_set_digest,
        "semanticContractSetDigest": fixture.revision.semantic_contract_set_digest(),
    }))
    .expect("publication canonical input");
    IdempotencyRequest::new(scope, key, &canonical).expect("publication idempotency")
}

fn revision_idempotency(
    fixture: &HistoricProviderWorkflowFixture,
    record: &WorkflowDefinitionRecord,
    key: &str,
) -> IdempotencyRequest {
    let canonical = serde_json::to_vec(&json!({
        "organizationId": fixture.organization_id,
        "workflowDefinitionId": fixture.definition.id,
        "expectedVersion": 1,
        "contentDigest": record.revision.contract.digest(),
        "payloadSetDigest": record.revision.payload_set_digest,
        "semanticContractSetDigest": record.revision.semantic_contract_set_digest(),
    }))
    .expect("revision canonical input");
    IdempotencyRequest::new(
        format!(
            "organizations/{}/workflow-definitions/{}/revisions",
            fixture.organization_id, fixture.definition.id
        ),
        key,
        &canonical,
    )
    .expect("revision idempotency")
}

fn goal_idempotency(
    fixture: &HistoricProviderWorkflowFixture,
    key: &str,
    contract: &WorkflowGoalContract,
) -> IdempotencyRequest {
    let canonical = serde_json::to_vec(&json!({
        "organizationId": fixture.organization_id,
        "projectId": fixture.project_id,
        "goalDigest": contract.digest(),
        "inputDigest": contract.input_digest(),
        "compilerRevision": WorkflowPlanCompiler::compiler_revision(&fixture.revision),
    }))
    .expect("Goal canonical input");
    IdempotencyRequest::new(
        format!(
            "organizations/{}/projects/{}/workflow-goals",
            fixture.organization_id, fixture.project_id
        ),
        key,
        &canonical,
    )
    .expect("Goal idempotency")
}

fn run_idempotency(
    fixture: &HistoricProviderWorkflowFixture,
    key: &str,
    timeout_seconds: u64,
) -> IdempotencyRequest {
    let canonical = serde_json::to_vec(&json!({
        "organizationId": fixture.organization_id,
        "projectId": fixture.project_id,
        "workflowGoalId": fixture.goal.id,
        "planRevisionId": fixture.plan_revision.id,
        "planDigest": fixture.plan_revision.digest,
        "timeoutSeconds": timeout_seconds,
    }))
    .expect("Run canonical input");
    IdempotencyRequest::new(
        format!(
            "organizations/{}/projects/{}/workflow-runs",
            fixture.organization_id, fixture.project_id
        ),
        key,
        &canonical,
    )
    .expect("Run idempotency")
}

fn successor_record(
    fixture: &HistoricProviderWorkflowFixture,
    name: &str,
) -> WorkflowDefinitionRecord {
    let contract = contract_with_name(fixture, name);
    let revision = WorkflowRevision::successor(
        &fixture.revision,
        WorkflowRevisionId::new(),
        contract.clone(),
        fixture.revision.payloads.clone(),
        fixture.principal_id,
        fixture.revision.created_at + Duration::seconds(1),
    )
    .expect("successor revision");
    let definition = fixture
        .definition
        .advance(
            1,
            contract.spec().name.clone(),
            contract.spec().description.clone(),
            revision.id,
            contract.digest().clone(),
            revision.created_at,
        )
        .expect("successor definition");
    WorkflowDefinitionRecord {
        definition,
        revision,
    }
}

fn contract_with_name(fixture: &HistoricProviderWorkflowFixture, name: &str) -> WorkflowContract {
    let mut spec = fixture.revision.contract.spec().clone();
    spec.name = name.into();
    WorkflowContract::from_spec(spec).expect("renamed Workflow contract")
}

fn workflow_acl_with_name(fixture: &HistoricProviderWorkflowFixture, name: &str) -> String {
    contract_with_name(fixture, name).canonical_acl().to_owned()
}

fn goal_contract_with_input(
    fixture: &HistoricProviderWorkflowFixture,
    input: serde_json::Value,
) -> WorkflowGoalContract {
    let mut spec = fixture.goal.contract.spec().clone();
    spec.input = input;
    WorkflowGoalContract::from_spec(spec).expect("Goal contract with drifted input")
}

fn payload_acls(fixture: &HistoricProviderWorkflowFixture) -> Vec<WorkflowPayloadAcl> {
    fixture
        .revision
        .payloads
        .iter()
        .map(|payload| WorkflowPayloadAcl {
            kind: payload.kind(),
            acl: payload.canonical_acl().to_owned(),
        })
        .collect()
}

fn definition_scope(fixture: &HistoricProviderWorkflowFixture) -> String {
    format!(
        "organizations/{}/projects/{}/workflow-definitions",
        fixture.organization_id, fixture.project_id
    )
}

fn assert_idempotency_conflict(error: ApplicationError) {
    assert_eq!(
        error,
        ApplicationError::Conflict("idempotency key reused with different input".into())
    );
}

fn assert_runtime_admission_error(error: ApplicationError) {
    let ApplicationError::Invalid(message) = error else {
        panic!("expected invalid runtime admission error, got {error:?}");
    };
    assert!(
        message.contains("has no admitted Cloud runtime dispatch port"),
        "unexpected runtime admission error: {message}"
    );
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
