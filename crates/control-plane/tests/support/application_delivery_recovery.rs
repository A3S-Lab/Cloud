use super::application_workflow_runs_support::{persist_ontology, persist_workflow};
use super::applications_support::{cqrs_contract, digest, seed_scope};
use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_control_plane::modules::applications::{
    AdmitApplicationInvocation, AdmitApplicationInvocationHandler, AdmitApplicationSession,
    AdmitApplicationSessionHandler, Application, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationMessage, ApplicationMessageKind, ApplicationRecord,
    ApplicationRelease, ApplicationReleasePublished, ApplicationResponseMode,
    ConversationVariableRevision, CreateApplicationWrite, IApplicationRepository,
    IApplicationSessionRepository, IApplicationWorkflowRevisionPort,
    IWorkflowApplicationEffectsPort, PostgresApplicationRepository,
    PostgresApplicationSessionRepository, ReplayApplicationSession,
    ReplayApplicationSessionHandler, WorkflowApplicationEffectsService,
    WorkflowApplicationMessageRequest, WorkflowApplicationOntologyRevisionReader,
    WorkflowApplicationRunReference, WorkflowApplicationRunService,
    WorkflowApplicationTerminalRequest, WorkflowApplicationVariableSnapshot,
    WorkflowApplicationVariableWriteRequest,
};
use a3s_cloud_control_plane::modules::connectors::{
    IWorkflowConnectorPort, WorkflowConnectorAttemptRequest, WorkflowConnectorAttemptResult,
};
use a3s_cloud_control_plane::modules::executions::{
    Execution, IWorkflowExecutionPort, WorkflowExecutionRequest,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::shared_kernel::application::{
    ApplicationError, ApplicationResult,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId,
    PlanRevisionId, PrincipalId, ProjectId, ResourceName, Sha256Digest, WorkflowDefinitionId,
    WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_cloud_control_plane::modules::workflow::domain::{
    WorkflowRevisionSemanticContracts, WorkflowStepDescriptorBinding,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorBindingsSpec,
};
use a3s_cloud_control_plane::modules::workflow::{
    CapabilityType, FlowWorkflowRunCoordinator, IWorkflowCompositeExecutionPort,
    IWorkflowRunCoordinator, IWorkflowRunRepository, IWorkflowRunVariableReader,
    PostgresOntologyRepository, PostgresWorkflowDefinitionRepository,
    PostgresWorkflowGoalRepository, PostgresWorkflowRunRepository,
    WorkflowCompositeExecutionRequest, WorkflowContract, WorkflowDataSchema, WorkflowDataType,
    WorkflowDefinition, WorkflowEdgeSpec, WorkflowGoalContract, WorkflowGoalSpec, WorkflowPayload,
    WorkflowPayloadContent, WorkflowPlanCompiler, WorkflowRevision, WorkflowRunCompiler,
    WorkflowRunFlowRuntime, WorkflowRunRecord, WorkflowRunStatus, WorkflowRunVariableReader,
    WorkflowSpec, WorkflowStepBindingKind, WorkflowStepConfiguration,
    WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistry,
    WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorSpec, WorkflowStepExecutionClass,
    WorkflowStepFailureContract, WorkflowStepFallbackMode, WorkflowStepKind, WorkflowStepOwner,
    WorkflowStepPort, WorkflowStepPortCardinality, WorkflowStepPresentationSpec,
    WorkflowStepRetryClassification, WorkflowStepSpec, WorkflowVariableAssignment,
    WorkflowVariableContract, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableMutationMode, WorkflowVariableScope, WorkflowVariableStorageClass,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION_V12, WORKFLOW_RUN_INPUT_SCHEMA_V12,
};
use a3s_flow::{
    FlowEngine, RuntimeBuildCompatibility, RuntimeBuildId, WorkflowSpec as FlowWorkflowSpec,
};
use a3s_orm::{Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const ANSWER_STEP_ID: &str = "answer";
const VARIABLE_STEP_ID: &str = "assign_conversation";

#[test]
fn application_delivery_recovery_fixture_is_a_valid_semantic_revision() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let actor = PrincipalId::new();
    let now = Utc::now();
    let revision = application_effect_workflow_revision(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        actor,
        now,
    );
    revision
        .validate()
        .expect("valid Application C6-C11 recovery Workflow revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        revision.contract.spec().name.clone(),
        revision.contract.spec().description.clone(),
        revision_id,
        revision.contract.digest().clone(),
        actor,
        now,
    )
    .expect("Application recovery Workflow definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract =
        a3s_cloud_control_plane::modules::workflow::OntologyContract::from_spec(
            a3s_cloud_control_plane::modules::workflow::OntologySpec {
                name: "Application recovery ontology".into(),
                description: String::new(),
                object_types: vec![
                    a3s_cloud_control_plane::modules::workflow::OntologyObjectType {
                        id: "request".into(),
                        label: "Request".into(),
                        schema_digest: digest('9'),
                        key_fields: vec!["ticketId".into()],
                    },
                ],
                relation_types: Vec::new(),
                rules: Vec::new(),
            },
        )
        .expect("Application recovery Ontology contract");
    let ontology = a3s_cloud_control_plane::modules::workflow::OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract.clone(),
        actor,
        now,
    );
    let goal = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Application recovery goal".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: revision.contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({
            "ticketId": "T-42",
            "topic": "billing",
            "conversationRevision": 999,
            "conversationEffect": "caller-value-must-not-authorize"
        }),
    })
    .expect("Application recovery Workflow goal");
    let compiled_goal = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        goal,
        &definition,
        &revision,
        &ontology,
        actor,
        now,
    )
    .expect("compile Application recovery goal");
    let compiled_run = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &compiled_goal.goal,
        &compiled_goal.plan_revision,
        &revision,
        Some(120),
        actor,
        now,
    )
    .expect("compile Application recovery WorkflowRun");
    assert_eq!(
        compiled_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V12
    );
    let projection = compiled_run
        .run
        .execution_input
        .application_projection
        .expect("Application recovery projection");
    assert_eq!(projection.answer_step_ids, [ANSWER_STEP_ID]);
    assert_eq!(projection.variable_step_ids, [VARIABLE_STEP_ID]);
    assert_eq!(projection.variable_assignment_step_ids, [VARIABLE_STEP_ID]);
}

pub(super) async fn exercise_application_delivery_recovery(
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
    let workflow_revision = application_effect_workflow_revision(
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id,
        actor,
        created_at,
    );
    persist_workflow(&executor, &workflow_revision).await?;
    let ontology_revision =
        persist_ontology(&executor, organization_id, project_id, actor, created_at).await?;

    let workflow_reader = a3s_cloud_control_plane::modules::applications::
        WorkflowApplicationReleaseEvidenceReader::new(Arc::new(
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
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        a3s_cloud_control_plane::modules::shared_kernel::domain::ApplicationId::new(),
        a3s_cloud_control_plane::modules::shared_kernel::domain::ApplicationReleaseId::new(),
        cqrs_contract(&workflow_evidence, '6'),
        actor,
        created_at,
    )?;
    let application = Application::create(
        release.application_id,
        ResourceName::parse("PostgreSQL recovery application")?,
        "C6-C11 command and Flow effect recovery over one Applications authority".into(),
        &release,
    )?;
    let application_request_id = Uuid::now_v7();
    PostgresApplicationRepository::new(executor.clone())
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(
                &application,
                &release,
                application_request_id,
            )?,
            actor_principal_id: actor,
            request_id: application_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-application-delivery-recovery",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )?,
            record: ApplicationRecord::new(application.clone(), release.clone())?,
        })
        .await?;

    let resource_access = ResourceAccessEvaluator::organization_wide();
    let opened = AdmitApplicationSessionHandler::new(
        Arc::new(PostgresApplicationRepository::new(executor.clone())),
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
    )
    .execute(
        AdmitApplicationSession {
            organization_id,
            project_id,
            application_id: application.id,
            release_id: release.id,
            initial_variables: json!({"locale": "en-US"}),
            actor_principal_id: actor,
            resource_access: resource_access.clone(),
            idempotency_key: "postgres-session".into(),
        },
        cqrs_context(),
    )
    .await?
    .map_err(|error| format!("admit Application session: {error}"))?;
    assert!(!opened.replayed);
    let replayed_open = AdmitApplicationSessionHandler::new(
        Arc::new(PostgresApplicationRepository::new(executor.clone())),
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
    )
    .execute(
        AdmitApplicationSession {
            organization_id,
            project_id,
            application_id: application.id,
            release_id: release.id,
            initial_variables: json!({"locale": "en-US"}),
            actor_principal_id: actor,
            resource_access: resource_access.clone(),
            idempotency_key: "postgres-session".into(),
        },
        cqrs_context(),
    )
    .await?
    .map_err(|error| format!("replay Application session after restart: {error}"))?;
    assert!(replayed_open.replayed);
    assert_eq!(replayed_open.session, opened.session);

    let invocation_command = AdmitApplicationInvocation {
        organization_id,
        project_id,
        application_id: application.id,
        session_id: opened.session.id,
        ontology_id: ontology_revision.ontology_id,
        ontology_revision_id: ontology_revision.id,
        environment_id: None,
        response_mode: ApplicationResponseMode::Streaming,
        input: json!({
            "ticketId": "T-42",
            "topic": "billing",
            "conversationRevision": 999,
            "conversationEffect": "caller-value-must-not-authorize"
        }),
        timeout_seconds: Some(120),
        actor_principal_id: actor,
        resource_access: resource_access.clone(),
        idempotency_key: "postgres-invocation".into(),
    };
    let admitted = invocation_handler(&executor)
        .execute(invocation_command.clone(), cqrs_context())
        .await?
        .map_err(|error| format!("admit Application invocation: {error}"))?;
    assert!(!admitted.replayed);
    assert_eq!(
        admitted.invocation.status,
        ApplicationInvocationStatus::Running
    );
    let replayed_invocation = invocation_handler(&executor)
        .execute(invocation_command, cqrs_context())
        .await?
        .map_err(|error| format!("replay Application invocation after restart: {error}"))?;
    assert!(replayed_invocation.replayed);
    assert_eq!(replayed_invocation.invocation, admitted.invocation);
    assert_eq!(replayed_invocation.workflow, admitted.workflow);

    let runs = PostgresWorkflowRunRepository::new(executor.clone());
    let record = runs
        .find(organization_id, admitted.workflow.workflow_run_id)
        .await?
        .expect("persisted Application WorkflowRun");
    let input = &record.run.execution_input;
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V12);
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V12);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application WorkflowRun projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
    );
    assert_eq!(projection.answer_step_ids, [ANSWER_STEP_ID]);
    assert_eq!(projection.variable_step_ids, [VARIABLE_STEP_ID]);
    assert_eq!(projection.variable_assignment_step_ids, [VARIABLE_STEP_ID]);

    let engine = start_flow(&record).await?;
    let answer_failure = Arc::new(RecoveringApplicationEffects::new(
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
        LostResponse::Answer,
    ));
    let answer_error = coordinator(engine.clone(), answer_failure)
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("lost Answer response must leave the Hook recoverable");
    assert!(answer_error.to_string().contains("lost Answer response"));
    assert_eq!(
        persisted_messages(&executor, &record, application.id)
            .await?
            .len(),
        2
    );

    let variable_failure = Arc::new(RecoveringApplicationEffects::new(
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
        LostResponse::Variables,
    ));
    let variable_error = coordinator(engine.clone(), variable_failure)
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("lost variable response must leave the CAS Hook recoverable");
    assert!(variable_error
        .to_string()
        .contains("lost variable response"));
    let committed_variables = WorkflowApplicationEffectsService::new(Arc::new(
        PostgresApplicationSessionRepository::new(executor.clone()),
    ))
    .read_conversation_variables(&WorkflowApplicationRunReference {
        organization_id,
        workflow_run_id: record.run.id,
    })
    .await?;
    assert_eq!(committed_variables.version.revision_number, 2);
    assert_eq!(
        committed_variables.values,
        json!({"conversation_topic": "billing", "locale": "en-US"})
    );

    let recovered_effects = Arc::new(RecoveringApplicationEffects::new(
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
        LostResponse::None,
    ));
    let recovered_coordinator = coordinator(engine.clone(), recovered_effects);
    let completed = recovered_coordinator
        .reconcile(&record, record.run.requested_at)
        .await?
        .expect("completed Application WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let replayed_projection = recovered_coordinator
        .reconcile(&record, record.run.requested_at)
        .await?
        .expect("lost projection-save replay");
    assert_eq!(replayed_projection, completed);
    let persisted = runs
        .save_projection(completed.clone(), record.run.aggregate_version)
        .await?;
    assert_eq!(persisted.run.status, WorkflowRunStatus::Completed);

    let messages = ReplayApplicationSessionHandler::new(Arc::new(
        PostgresApplicationSessionRepository::new(executor.clone()),
    ))
    .execute(
        ReplayApplicationSession {
            organization_id,
            project_id,
            application_id: application.id,
            session_id: opened.session.id,
            after_sequence: 0,
            limit: Some(10),
            actor_principal_id: actor,
            resource_access,
        },
        cqrs_context(),
    )
    .await?
    .map_err(|error| format!("replay completed Application session: {error}"))?;
    assert_eq!(messages.messages.len(), 3);
    assert_eq!(messages.messages[0].kind, ApplicationMessageKind::Input);
    assert_eq!(messages.messages[1].kind, ApplicationMessageKind::Answer);
    assert_eq!(
        messages.messages[2].kind,
        ApplicationMessageKind::FinalOutput
    );

    let invocation = PostgresApplicationSessionRepository::new(executor.clone())
        .find_invocation(
            organization_id,
            project_id,
            application.id,
            admitted.invocation.id,
        )
        .await?
        .expect("completed Application invocation");
    assert_eq!(invocation.status, ApplicationInvocationStatus::Succeeded);
    let inspection = WorkflowRunVariableReader::new(engine)
        .inspect(&completed)
        .await?;
    let conversation_topic = inspection
        .variables
        .iter()
        .find(|variable| variable.name == "conversation_topic")
        .expect("Application conversation variable inspection");
    assert_eq!(conversation_topic.value, Some(json!("billing")));

    let counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from application_messages where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_conversation_variable_revisions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_workflow_effect_claims where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from workflow_runs where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_invocations where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(")"),
        )
        .await?;
    assert_eq!(counts, (3, 2, 3, 1, 1));
    Ok(())
}

#[derive(Clone, Copy)]
enum LostResponse {
    None,
    Answer,
    Variables,
}

struct RecoveringApplicationEffects {
    inner: WorkflowApplicationEffectsService,
    lose_answer: AtomicBool,
    lose_variables: AtomicBool,
}

impl RecoveringApplicationEffects {
    fn new(sessions: Arc<dyn IApplicationSessionRepository>, failure: LostResponse) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            lose_answer: AtomicBool::new(matches!(failure, LostResponse::Answer)),
            lose_variables: AtomicBool::new(matches!(failure, LostResponse::Variables)),
        }
    }
}

#[async_trait]
impl IWorkflowApplicationEffectsPort for RecoveringApplicationEffects {
    async fn read_conversation_variables(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationVariableSnapshot> {
        self.inner.read_conversation_variables(reference).await
    }

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        let committed = self.inner.append_answer(request).await?;
        if self.lose_answer.swap(false, Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "injected lost Answer response after PostgreSQL commit".into(),
            ));
        }
        Ok(committed)
    }

    async fn append_final_output(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.inner.append_final_output(request).await
    }

    async fn advance_conversation_variables(
        &self,
        request: &WorkflowApplicationVariableWriteRequest,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
        let committed = self.inner.advance_conversation_variables(request).await?;
        if self.lose_variables.swap(false, Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "injected lost variable response after PostgreSQL commit".into(),
            ));
        }
        Ok(committed)
    }

    async fn observe_terminal(
        &self,
        request: &WorkflowApplicationTerminalRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>> {
        self.inner.observe_terminal(request).await
    }
}

struct UnusedExecutionPort;

#[async_trait]
impl IWorkflowExecutionPort for UnusedExecutionPort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowExecutionRequest,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }
}

struct UnusedCompositePort;

#[async_trait]
impl IWorkflowCompositeExecutionPort for UnusedCompositePort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
        _reason: Option<String>,
        _requested_by: PrincipalId,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }
}

struct UnusedConnectorPort;

#[async_trait]
impl IWorkflowConnectorPort for UnusedConnectorPort {
    async fn execute_attempt(
        &self,
        _request: &WorkflowConnectorAttemptRequest,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Connector port".into(),
        ))
    }
}

fn coordinator(
    engine: FlowEngine,
    effects: Arc<dyn IWorkflowApplicationEffectsPort>,
) -> FlowWorkflowRunCoordinator {
    FlowWorkflowRunCoordinator::with_all_ports_and_application_effects(
        engine,
        Arc::new(UnusedExecutionPort),
        Arc::new(UnusedCompositePort),
        Arc::new(UnusedConnectorPort),
        effects,
    )
}

fn invocation_handler(executor: &PostgresExecutor) -> AdmitApplicationInvocationHandler {
    AdmitApplicationInvocationHandler::new(
        Arc::new(PostgresApplicationRepository::new(executor.clone())),
        Arc::new(PostgresApplicationSessionRepository::new(executor.clone())),
        Arc::new(WorkflowApplicationOntologyRevisionReader::new(Arc::new(
            PostgresOntologyRepository::new(executor.clone()),
        ))),
        Arc::new(PostgresProjectsRepository::new(executor.clone())),
        Arc::new(WorkflowApplicationRunService::new(
            Arc::new(PostgresWorkflowDefinitionRepository::new(executor.clone())),
            Arc::new(PostgresOntologyRepository::new(executor.clone())),
            Arc::new(PostgresWorkflowGoalRepository::new(executor.clone())),
            Arc::new(PostgresWorkflowRunRepository::new(executor.clone())),
        )),
    )
}

async fn start_flow(record: &WorkflowRunRecord) -> Result<FlowEngine, Box<dyn std::error::Error>> {
    let runtime_build_id = RuntimeBuildId::new("a3s-cloud-postgres-application-recovery@1")?;
    let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime::default()))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            record.run.flow_run_id.clone(),
            FlowWorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V12,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(&record.run.execution_input)?,
        )
        .await?;
    Ok(engine)
}

async fn persisted_messages(
    executor: &PostgresExecutor,
    record: &WorkflowRunRecord,
    application_id: a3s_cloud_control_plane::modules::shared_kernel::domain::ApplicationId,
) -> Result<Vec<ApplicationMessage>, Box<dyn std::error::Error>> {
    let repository = PostgresApplicationSessionRepository::new(executor.clone());
    let invocation = repository
        .find_invocation_for_workflow_run(record.run.organization_id, record.run.id)
        .await?
        .expect("Application invocation bound to WorkflowRun");
    repository
        .list_messages(
            record.run.organization_id,
            record.run.project_id,
            application_id,
            invocation.session_id,
            0,
            10,
        )
        .await
        .map_err(Into::into)
}

fn application_effect_workflow_revision(
    organization_id: OrganizationId,
    project_id: ProjectId,
    definition_id: WorkflowDefinitionId,
    revision_id: WorkflowRevisionId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> WorkflowRevision {
    let schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))
        .expect("Application recovery data schema");
    let input_configuration = configuration(WorkflowStepKind::Input);
    let output_configuration = configuration(WorkflowStepKind::Output);
    let variable_configuration = configuration(WorkflowStepKind::Service);
    let workflow = WorkflowSpec {
        name: "Application PostgreSQL recovery".into(),
        description: "Exact Answer, variable CAS, final output, and terminal effect chain".into(),
        steps: vec![
            workflow_step(
                "input",
                WorkflowStepKind::Input,
                input_configuration.digest().clone(),
                schema.digest().clone(),
            ),
            workflow_step(
                ANSWER_STEP_ID,
                WorkflowStepKind::Output,
                output_configuration.digest().clone(),
                schema.digest().clone(),
            ),
            workflow_step(
                VARIABLE_STEP_ID,
                WorkflowStepKind::Service,
                variable_configuration.digest().clone(),
                schema.digest().clone(),
            ),
            workflow_step(
                "output",
                WorkflowStepKind::Output,
                output_configuration.digest().clone(),
                schema.digest().clone(),
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-answer".into(),
                source: "input".into(),
                target: ANSWER_STEP_ID.into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "input-assign".into(),
                source: "input".into(),
                target: VARIABLE_STEP_ID.into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "assign-output".into(),
                source: VARIABLE_STEP_ID.into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    let mut answer = descriptor(
        "application.answer",
        WorkflowStepKind::Output,
        "content",
        "message",
    );
    answer.owner = WorkflowStepOwner::Applications;
    answer.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    answer.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    let mut variable = descriptor(
        "application.conversation-variable-assign",
        WorkflowStepKind::Service,
        "input",
        "values",
    );
    variable.owner = WorkflowStepOwner::Applications;
    variable.execution_class = WorkflowStepExecutionClass::OwningApplicationPort;
    variable.required_bindings = vec![WorkflowStepBindingKind::ReleaseReference];
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "integration.application-recovery".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
            ),
            answer,
            variable,
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
            ),
        ],
    })
    .expect("Application recovery descriptor registry");
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "integration.application-recovery".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [
            ("input", "workflow.input"),
            (ANSWER_STEP_ID, "application.answer"),
            (VARIABLE_STEP_ID, "application.conversation-variable-assign"),
            ("output", "workflow.output"),
        ]
        .into_iter()
        .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
            step_id: step_id.into(),
            descriptor_id: descriptor_id.into(),
            descriptor_revision: "1.0.0".into(),
            semantic_digest: registry
                .resolve(descriptor_id, "1.0.0")
                .expect("Application recovery descriptor")
                .semantic_digest()
                .clone(),
        })
        .collect(),
    })
    .expect("Application recovery descriptor bindings");
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "integration.application-recovery".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            WorkflowVariableDeclaration {
                name: "request".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Any,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: Some(schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_topic".into(),
                scope: WorkflowVariableScope::Application,
                value_type: WorkflowDataType::String,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: None,
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::OptimisticApplicationPort,
                required: false,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_revision".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Number,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: Some(schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: vec!["conversationRevision".into()],
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "conversation_effect".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::String,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: Some(schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: vec!["conversationEffect".into()],
                region_id: None,
                default_value_digest: None,
            },
        ],
        reads: Vec::new(),
        assignments: vec![WorkflowVariableAssignment {
            id: "assign-conversation-topic".into(),
            target_variable: "conversation_topic".into(),
            source_variable: "request".into(),
            writer_step_id: VARIABLE_STEP_ID.into(),
            writer_region_id: None,
            source_path: vec!["topic".into()],
            value_type: WorkflowDataType::String,
            value_schema_digest: schema.digest().clone(),
            mutation_order: 1,
            expected_revision_variable: Some("conversation_revision".into()),
            idempotency_key_variable: Some("conversation_effect".into()),
        }],
        exports: Vec::new(),
    })
    .expect("Application recovery variable contract");
    let semantic_contracts =
        WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variables)
            .expect("Application recovery semantic contracts");
    WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        WorkflowContract::from_spec(workflow).expect("Application recovery Workflow contract"),
        vec![
            schema,
            input_configuration,
            variable_configuration,
            output_configuration,
        ],
        semantic_contracts,
        actor,
        created_at,
    )
    .expect("Application recovery Workflow revision")
}

fn workflow_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration_digest: Sha256Digest,
    schema_digest: Sha256Digest,
) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest,
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest,
        policy_digest: None,
        capability: None,
    }
}

fn configuration(kind: WorkflowStepKind) -> WorkflowPayload {
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(kind),
    ))
    .expect("Application recovery step configuration")
}

fn descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_port: &str,
    output_port: &str,
) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: id.into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port(input_port)],
        output_ports: vec![port(output_port)],
        configuration_schema_digest: digest('c'),
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::<CapabilityType>::new(),
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: id.into(),
            summary: format!("{id} PostgreSQL recovery descriptor"),
            icon_key: id.into(),
        },
    }
}

fn port(name: &str) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type: WorkflowDataType::Any,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}

fn cqrs_context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
