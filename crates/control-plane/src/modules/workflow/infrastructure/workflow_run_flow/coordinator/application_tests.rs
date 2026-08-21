use super::{application_lifecycle_projection, FlowWorkflowRunCoordinator};
use crate::modules::applications::{
    AdvanceApplicationInvocationWrite, ApplicationAudience, ApplicationDeliveryPolicy,
    ApplicationEndUser, ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationMessageKind, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
    ApplicationWorkflowBinding, ConversationVariableRevision, IApplicationSessionRepository,
    IWorkflowApplicationEffectsPort, InMemoryApplicationSessionRepository,
    OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
    WorkflowApplicationEffectRequest, WorkflowApplicationEffectsService,
    WorkflowApplicationMessageRequest, WorkflowApplicationRunReference,
    WorkflowApplicationTerminalRequest, WorkflowApplicationVariableSnapshot,
    WorkflowApplicationVariableWriteRequest,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, EnvironmentId, IdempotentWrite, OntologyId, OntologyRevisionId,
    PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowRun, WorkflowRunFlowState, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_FLOW_VERSION_V10, WORKFLOW_RUN_FLOW_VERSION_V11,
    WORKFLOW_RUN_FLOW_VERSION_V12,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{
    application_answer_workflow_run_input, application_variable_workflow_run_input,
    application_workflow_run_input, workflow_run_input, TEST_APPLICATION_VARIABLE_STEP_ID,
};
use a3s_flow::{FlowEngine, RuntimeBuildCompatibility, RuntimeBuildId, WorkflowSpec};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedApplicationEffect {
    VariableSnapshot(WorkflowApplicationRunReference),
    Variables(WorkflowApplicationVariableWriteRequest),
    Answer(WorkflowApplicationMessageRequest),
    FinalOutput(WorkflowApplicationMessageRequest),
    Terminal(WorkflowApplicationTerminalRequest),
}

struct RecordingApplicationEffects {
    inner: WorkflowApplicationEffectsService,
    calls: Mutex<Vec<RecordedApplicationEffect>>,
    lose_answer_response_once: AtomicBool,
    drift_answer_evidence_once: AtomicBool,
    lose_variable_response_once: AtomicBool,
    drift_variable_evidence_once: AtomicBool,
}

impl RecordingApplicationEffects {
    fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
            lose_answer_response_once: AtomicBool::new(false),
            drift_answer_evidence_once: AtomicBool::new(false),
            lose_variable_response_once: AtomicBool::new(false),
            drift_variable_evidence_once: AtomicBool::new(false),
        }
    }

    fn with_lost_answer_response(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
            lose_answer_response_once: AtomicBool::new(true),
            drift_answer_evidence_once: AtomicBool::new(false),
            lose_variable_response_once: AtomicBool::new(false),
            drift_variable_evidence_once: AtomicBool::new(false),
        }
    }

    fn with_drifted_answer_evidence(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
            lose_answer_response_once: AtomicBool::new(false),
            drift_answer_evidence_once: AtomicBool::new(true),
            lose_variable_response_once: AtomicBool::new(false),
            drift_variable_evidence_once: AtomicBool::new(false),
        }
    }

    fn with_lost_variable_response(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
            lose_answer_response_once: AtomicBool::new(false),
            drift_answer_evidence_once: AtomicBool::new(false),
            lose_variable_response_once: AtomicBool::new(true),
            drift_variable_evidence_once: AtomicBool::new(false),
        }
    }

    fn with_drifted_variable_evidence(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
            lose_answer_response_once: AtomicBool::new(false),
            drift_answer_evidence_once: AtomicBool::new(false),
            lose_variable_response_once: AtomicBool::new(false),
            drift_variable_evidence_once: AtomicBool::new(true),
        }
    }

    async fn calls(&self) -> Vec<RecordedApplicationEffect> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl IWorkflowApplicationEffectsPort for RecordingApplicationEffects {
    async fn read_conversation_variables(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationVariableSnapshot> {
        self.calls
            .lock()
            .await
            .push(RecordedApplicationEffect::VariableSnapshot(
                reference.clone(),
            ));
        self.inner.read_conversation_variables(reference).await
    }

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.calls
            .lock()
            .await
            .push(RecordedApplicationEffect::Answer(request.clone()));
        let mut write = self.inner.append_answer(request).await?;
        if self.lose_answer_response_once.swap(false, Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "injected loss after Answer commit".into(),
            ));
        }
        if self
            .drift_answer_evidence_once
            .swap(false, Ordering::SeqCst)
        {
            write.value.created_at += Duration::milliseconds(1);
        }
        Ok(write)
    }

    async fn append_final_output(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.calls
            .lock()
            .await
            .push(RecordedApplicationEffect::FinalOutput(request.clone()));
        self.inner.append_final_output(request).await
    }

    async fn advance_conversation_variables(
        &self,
        request: &WorkflowApplicationVariableWriteRequest,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
        self.calls
            .lock()
            .await
            .push(RecordedApplicationEffect::Variables(request.clone()));
        let mut write = self.inner.advance_conversation_variables(request).await?;
        if self
            .lose_variable_response_once
            .swap(false, Ordering::SeqCst)
        {
            return Err(ApplicationError::Unavailable(
                "injected loss after Application variable commit".into(),
            ));
        }
        if self
            .drift_variable_evidence_once
            .swap(false, Ordering::SeqCst)
        {
            write.value.created_at += Duration::milliseconds(1);
        }
        Ok(write)
    }

    async fn observe_terminal(
        &self,
        request: &WorkflowApplicationTerminalRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>> {
        self.calls
            .lock()
            .await
            .push(RecordedApplicationEffect::Terminal(request.clone()));
        self.inner.observe_terminal(request).await
    }
}

struct ApplicationBindingFixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    effects: Arc<RecordingApplicationEffects>,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
}

async fn bind_application_invocation(
    record: &WorkflowRunRecord,
    actor: PrincipalId,
) -> ApplicationBindingFixture {
    let organization_id = record.run.organization_id;
    let project_id = record.run.project_id;
    let application_id = ApplicationId::new();
    let requested_at = record.run.requested_at;
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatflow,
            audience: ApplicationAudience::ProjectMembers,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationInteractionMode::Conversation,
                response_modes: vec![ApplicationResponseMode::Streaming],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::new(),
                workflow_revision_id: WorkflowRevisionId::new(),
                workflow_contract_digest: digest('a'),
                workflow_payload_set_digest: digest('b'),
                workflow_semantic_contract_set_digest: digest('c'),
                input_schema_digest: digest('d'),
                output_schema_digest: digest('e'),
            },
            presentation_digest: digest('f'),
        })
        .expect("release contract"),
        actor,
        requested_at - Duration::seconds(3),
    )
    .expect("release");
    let end_user =
        ApplicationEndUser::project_member(&release, actor, requested_at - Duration::seconds(2))
            .expect("end user");
    let session_id = ApplicationSessionId::new();
    let initial_variables = ConversationVariableRevision::initial(
        session_id,
        &release,
        json!({"locale": "en-US"}),
        requested_at - Duration::seconds(1),
    )
    .expect("initial variables");
    let session = ApplicationSession::create(
        session_id,
        &release,
        &end_user,
        &initial_variables,
        requested_at - Duration::seconds(1),
    )
    .expect("session");
    let sessions = Arc::new(InMemoryApplicationSessionRepository::new());
    sessions
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables,
        })
        .await
        .expect("open session");

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        requested_at,
    )
    .expect("invocation");
    let input_message =
        ApplicationMessage::input(&session, &invocation, requested_at).expect("input message");
    let workflow_authority = ApplicationInvocationWorkflowAuthority::new(
        &invocation,
        OntologyId::new(),
        OntologyRevisionId::new(),
        digest('1'),
        Some(EnvironmentId::new()),
        actor,
        3_600,
    )
    .expect("Workflow authority");
    sessions
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: invocation.clone(),
            workflow_authority,
            input_message,
            expected_session_version: session.aggregate_version,
        })
        .await
        .expect("request invocation");
    let running = invocation
        .bind_workflow_run(invocation.aggregate_version, record.run.id, requested_at)
        .expect("bind WorkflowRun");
    sessions
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: running,
            expected_version: invocation.aggregate_version,
        })
        .await
        .expect("persist WorkflowRun binding");
    let effects = Arc::new(RecordingApplicationEffects::new(sessions.clone()));
    ApplicationBindingFixture {
        sessions,
        effects,
        application_id,
        session_id,
    }
}

async fn application_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, PrincipalId) {
    let mut input = application_workflow_run_input().expect("Application WorkflowRun input");
    let requested_at = canonical_timestamp(Utc::now());
    input.organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
    input.project_id = ProjectId::new();
    input.workflow_run_id = WorkflowRunId::new();
    input.requested_at = requested_at;
    input.deadline_at = requested_at + Duration::hours(1);
    input
        .validate()
        .expect("valid Application WorkflowRun input");
    let actor = PrincipalId::new();
    let (run, steps) = WorkflowRun::create(input.clone(), actor).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-application-lifecycle-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime::default()))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V10,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start Application WorkflowRun Flow");
    (engine, record, actor)
}

async fn application_answer_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, PrincipalId) {
    let mut input =
        application_answer_workflow_run_input().expect("Application Answer WorkflowRun input");
    let requested_at = canonical_timestamp(Utc::now());
    input.organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
    input.project_id = ProjectId::new();
    input.workflow_run_id = WorkflowRunId::new();
    input.requested_at = requested_at;
    input.deadline_at = requested_at + Duration::hours(1);
    input
        .validate()
        .expect("valid Application Answer WorkflowRun input");
    let actor = PrincipalId::new();
    let (run, steps) = WorkflowRun::create(input.clone(), actor).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-application-answer-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime::default()))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V11,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start Application Answer WorkflowRun Flow");
    (engine, record, actor)
}

async fn application_variable_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, PrincipalId) {
    let mut input =
        application_variable_workflow_run_input().expect("Application variable WorkflowRun input");
    let requested_at = canonical_timestamp(Utc::now());
    input.organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
    input.project_id = ProjectId::new();
    input.workflow_run_id = WorkflowRunId::new();
    input.requested_at = requested_at;
    input.deadline_at = requested_at + Duration::hours(1);
    input
        .validate()
        .expect("valid Application variable WorkflowRun input");
    let actor = PrincipalId::new();
    let (run, steps) = WorkflowRun::create(input.clone(), actor).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-application-variable-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime::default()))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V12,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start Application variable WorkflowRun Flow");
    (engine, record, actor)
}

#[tokio::test]
async fn variable_snapshot_and_cas_commit_precede_final_output_and_terminal_projection() {
    let (engine, record, actor) = application_variable_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine, binding.effects.clone());

    let waiting = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application variable snapshot reconciliation")
        .expect("waiting WorkflowRun projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert!(matches!(
        binding.effects.calls().await.as_slice(),
        [RecordedApplicationEffect::VariableSnapshot(_)]
    ));

    let completed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application variable write reconciliation")
        .expect("completed WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let calls = binding.effects.calls().await;
    assert!(matches!(
        calls.as_slice(),
        [
            RecordedApplicationEffect::VariableSnapshot(_),
            RecordedApplicationEffect::Variables(_),
            RecordedApplicationEffect::FinalOutput(_),
            RecordedApplicationEffect::Terminal(_),
        ]
    ));
    let RecordedApplicationEffect::Variables(variable_write) = &calls[1] else {
        unreachable!("ordered Application variable write")
    };
    assert_eq!(
        variable_write.effect.step_id,
        TEST_APPLICATION_VARIABLE_STEP_ID
    );
    assert_eq!(variable_write.effect.step_attempt, 1);
    assert_eq!(variable_write.effect.effect_ordinal, 0);
    assert_eq!(
        variable_write.values,
        json!({"conversation_topic": "high", "locale": "en-US"})
    );
    let assignment = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_APPLICATION_VARIABLE_STEP_ID)
        .expect("Application variable step projection");
    assert_eq!(assignment.status, WorkflowStepProjectionStatus::Completed);
    assert_eq!(assignment.result.as_ref(), Some(&variable_write.values));

    let variables = binding
        .effects
        .inner
        .read_conversation_variables(&WorkflowApplicationRunReference {
            organization_id: record.run.organization_id,
            workflow_run_id: record.run.id,
        })
        .await
        .expect("committed Application variables");
    assert_eq!(variables.version.revision_number, 2);
    assert_eq!(variables.values, variable_write.values);

    let replayed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("lost-save replay")
        .expect("replayed WorkflowRun projection");
    assert_eq!(replayed, completed);
    assert_eq!(binding.effects.calls().await.len(), 6);
}

#[tokio::test]
async fn active_application_variable_hook_fails_closed_without_the_effect_port() {
    let (engine, record, _) = application_variable_workflow_fixture().await;
    let error = FlowWorkflowRunCoordinator::new(engine.clone())
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("missing Applications effect port");
    assert!(error.to_string().contains("not configured"));
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot");
    assert!(snapshot.hooks.values().any(|hook| {
        hook.status == a3s_flow::HookStatus::Active
            && hook
                .hook_id
                .starts_with("workflow-application-variable-snapshot:")
    }));
}

#[tokio::test]
async fn lost_application_variable_commit_response_replays_the_exact_cas_effect() {
    let (engine, record, actor) = application_variable_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let effects = Arc::new(RecordingApplicationEffects::with_lost_variable_response(
        binding.sessions.clone(),
    ));
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine.clone(), effects.clone());

    coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application variable snapshot");
    let error = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("lost Application variable response");
    assert!(error.to_string().contains("injected loss"));
    let waiting = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("waiting Flow snapshot");
    assert!(waiting.hooks.values().any(|hook| {
        hook.status == a3s_flow::HookStatus::Active
            && hook
                .hook_id
                .starts_with("workflow-application-variable-write:")
    }));
    let committed = effects
        .inner
        .read_conversation_variables(&WorkflowApplicationRunReference {
            organization_id: record.run.organization_id,
            workflow_run_id: record.run.id,
        })
        .await
        .expect("committed Application variables");
    assert_eq!(committed.version.revision_number, 2);

    let completed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application variable recovery")
        .expect("completed WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let calls = effects.calls().await;
    let [RecordedApplicationEffect::VariableSnapshot(_), RecordedApplicationEffect::Variables(first), RecordedApplicationEffect::Variables(replay), RecordedApplicationEffect::FinalOutput(_), RecordedApplicationEffect::Terminal(_)] =
        calls.as_slice()
    else {
        panic!("unexpected recovered effect order: {calls:?}")
    };
    assert_eq!(first, replay);
    let recovered = effects
        .inner
        .read_conversation_variables(&WorkflowApplicationRunReference {
            organization_id: record.run.organization_id,
            workflow_run_id: record.run.id,
        })
        .await
        .expect("recovered Application variables");
    assert_eq!(recovered.version.revision_number, 2);
    assert_eq!(recovered.values, replay.values);
}

#[tokio::test]
async fn drifted_application_variable_commit_evidence_leaves_write_hook_unresolved() {
    let (engine, record, actor) = application_variable_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let effects = Arc::new(RecordingApplicationEffects::with_drifted_variable_evidence(
        binding.sessions.clone(),
    ));
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine.clone(), effects.clone());

    coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application variable snapshot");
    let error = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("drifted Application variable evidence");
    assert!(error.to_string().contains("commit evidence drifted"));
    let waiting = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("waiting Flow snapshot");
    assert!(waiting.hooks.values().any(|hook| {
        hook.status == a3s_flow::HookStatus::Active
            && hook
                .hook_id
                .starts_with("workflow-application-variable-write:")
    }));
    assert!(matches!(
        effects.calls().await.as_slice(),
        [
            RecordedApplicationEffect::VariableSnapshot(_),
            RecordedApplicationEffect::Variables(_),
        ]
    ));
}

#[tokio::test]
async fn answer_commit_precedes_final_output_and_terminal_projection() {
    let (engine, record, actor) = application_answer_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine, binding.effects.clone());

    let completed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application Answer reconciliation")
        .expect("completed WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let calls = binding.effects.calls().await;
    assert!(matches!(
        calls.as_slice(),
        [
            RecordedApplicationEffect::Answer(_),
            RecordedApplicationEffect::FinalOutput(_),
            RecordedApplicationEffect::Terminal(_),
        ]
    ));
    let RecordedApplicationEffect::Answer(answer) = &calls[0] else {
        unreachable!("ordered Answer call")
    };
    assert_eq!(answer.effect.step_id, "answer");
    assert_eq!(answer.effect.step_attempt, 1);
    assert_eq!(answer.effect.effect_ordinal, 0);
    assert_eq!(answer.content, json!("HIGH T-42"));
    assert_ne!(
        answer.content,
        completed.run.output.clone().expect("output")
    );

    let messages = binding
        .sessions
        .list_messages(
            record.run.organization_id,
            record.run.project_id,
            binding.application_id,
            binding.session_id,
            0,
            10,
        )
        .await
        .expect("messages");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].kind, ApplicationMessageKind::Answer);
    assert_eq!(messages[2].kind, ApplicationMessageKind::FinalOutput);

    let replayed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("lost-save replay")
        .expect("replayed WorkflowRun projection");
    assert_eq!(replayed, completed);
    assert_eq!(binding.effects.calls().await.len(), 5);
    assert_eq!(
        binding
            .sessions
            .list_messages(
                record.run.organization_id,
                record.run.project_id,
                binding.application_id,
                binding.session_id,
                0,
                10,
            )
            .await
            .expect("replayed messages")
            .len(),
        3
    );
}

#[tokio::test]
async fn active_answer_fails_closed_without_the_application_effect_port() {
    let (engine, record, _) = application_answer_workflow_fixture().await;
    let error = FlowWorkflowRunCoordinator::new(engine.clone())
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("missing Applications effect port");
    assert!(error.to_string().contains("not configured"));
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot");
    assert!(snapshot
        .hooks
        .values()
        .any(|hook| hook.status == a3s_flow::HookStatus::Active));
}

#[tokio::test]
async fn lost_answer_commit_response_replays_the_exact_effect_before_resuming_flow() {
    let (engine, record, actor) = application_answer_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let effects = Arc::new(RecordingApplicationEffects::with_lost_answer_response(
        binding.sessions.clone(),
    ));
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine.clone(), effects.clone());

    let error = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("lost Answer response");
    assert!(error.to_string().contains("injected loss"));
    let waiting = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("waiting Flow snapshot");
    assert!(waiting
        .hooks
        .values()
        .any(|hook| hook.status == a3s_flow::HookStatus::Active));
    assert_eq!(
        binding
            .sessions
            .list_messages(
                record.run.organization_id,
                record.run.project_id,
                binding.application_id,
                binding.session_id,
                0,
                10,
            )
            .await
            .expect("committed Answer")
            .len(),
        2
    );

    let completed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Answer recovery")
        .expect("completed WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let calls = effects.calls().await;
    let [RecordedApplicationEffect::Answer(first), RecordedApplicationEffect::Answer(replay), RecordedApplicationEffect::FinalOutput(_), RecordedApplicationEffect::Terminal(_)] =
        calls.as_slice()
    else {
        panic!("unexpected recovered effect order: {calls:?}")
    };
    assert_eq!(first, replay);
    assert_eq!(
        binding
            .sessions
            .list_messages(
                record.run.organization_id,
                record.run.project_id,
                binding.application_id,
                binding.session_id,
                0,
                10,
            )
            .await
            .expect("recovered messages")
            .len(),
        3
    );
}

#[tokio::test]
async fn drifted_answer_commit_evidence_leaves_the_flow_hook_unresolved() {
    let (engine, record, actor) = application_answer_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let effects = Arc::new(RecordingApplicationEffects::with_drifted_answer_evidence(
        binding.sessions.clone(),
    ));
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine.clone(), effects.clone());

    let error = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("drifted Answer evidence");
    assert!(error.to_string().contains("commit evidence drifted"));
    let waiting = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("waiting Flow snapshot");
    assert!(waiting
        .hooks
        .values()
        .any(|hook| hook.status == a3s_flow::HookStatus::Active));
    assert_eq!(
        binding
            .sessions
            .list_messages(
                record.run.organization_id,
                record.run.project_id,
                binding.application_id,
                binding.session_id,
                0,
                10,
            )
            .await
            .expect("committed Answer")
            .len(),
        2
    );
}

#[tokio::test]
async fn completed_application_run_projects_final_output_before_terminal_and_replays_exactly() {
    let (engine, record, actor) = application_workflow_fixture().await;
    let binding = bind_application_invocation(&record, actor).await;
    let coordinator =
        FlowWorkflowRunCoordinator::with_application_effects(engine, binding.effects.clone());

    let completed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("Application lifecycle projection")
        .expect("completed WorkflowRun projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(
        binding.effects.calls().await,
        vec![
            RecordedApplicationEffect::FinalOutput(WorkflowApplicationMessageRequest {
                effect: WorkflowApplicationEffectRequest {
                    organization_id: completed.run.organization_id,
                    workflow_run_id: completed.run.id,
                    step_id: "output".into(),
                    step_attempt: 1,
                    effect_ordinal: 0,
                    occurred_at: completed.run.finished_at.expect("finish time"),
                },
                content: completed.run.output.clone().expect("Workflow output"),
            }),
            RecordedApplicationEffect::Terminal(WorkflowApplicationTerminalRequest {
                organization_id: completed.run.organization_id,
                workflow_run_id: completed.run.id,
                status: ApplicationInvocationStatus::Succeeded,
                completed_at: completed.run.finished_at.expect("finish time"),
            }),
        ]
    );

    let replayed = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("lost-save replay")
        .expect("replayed WorkflowRun projection");
    assert_eq!(replayed, completed);
    assert_eq!(binding.effects.calls().await.len(), 4);
    let invocation = binding
        .sessions
        .find_invocation_for_workflow_run(record.run.organization_id, record.run.id)
        .await
        .expect("invocation read")
        .expect("bound invocation");
    assert_eq!(invocation.status, ApplicationInvocationStatus::Succeeded);
    let messages = binding
        .sessions
        .list_messages(
            record.run.organization_id,
            record.run.project_id,
            binding.application_id,
            binding.session_id,
            0,
            10,
        )
        .await
        .expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].kind, ApplicationMessageKind::FinalOutput);
    assert_eq!(messages[1].content, completed.run.output.expect("output"));
}

#[tokio::test]
async fn application_completion_fails_closed_without_the_effect_port() {
    let (engine, record, _) = application_workflow_fixture().await;
    let error = FlowWorkflowRunCoordinator::new(engine)
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("missing Applications effect port");
    assert!(error.to_string().contains("not configured"));
}

#[tokio::test]
async fn missing_application_binding_stops_before_terminal_observation() {
    let (engine, record, _) = application_workflow_fixture().await;
    let sessions = Arc::new(InMemoryApplicationSessionRepository::new());
    let effects = Arc::new(RecordingApplicationEffects::new(sessions));
    let coordinator = FlowWorkflowRunCoordinator::with_application_effects(engine, effects.clone());
    let error = coordinator
        .reconcile(&record, record.run.requested_at)
        .await
        .expect_err("missing Application invocation binding");
    assert!(error.to_string().contains("not found"));
    assert!(matches!(
        effects.calls().await.as_slice(),
        [RecordedApplicationEffect::FinalOutput(_)]
    ));
}

#[tokio::test]
async fn historic_workflow_completion_never_requires_the_application_port() {
    let mut input = workflow_run_input().expect("historic WorkflowRun input");
    let requested_at = canonical_timestamp(Utc::now());
    input.requested_at = requested_at;
    input.deadline_at = requested_at + Duration::hours(1);
    input.validate().expect("valid historic WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-historic-lifecycle-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime::default()))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start historic WorkflowRun Flow");
    let completed = FlowWorkflowRunCoordinator::new(engine)
        .reconcile(&record, record.run.requested_at)
        .await
        .expect("historic reconciliation")
        .expect("historic terminal projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
}

#[test]
fn failed_timed_out_and_cancelled_runs_map_to_closed_application_statuses() {
    for (workflow_status, application_status) in [
        (
            WorkflowRunStatus::Failed,
            ApplicationInvocationStatus::Failed,
        ),
        (
            WorkflowRunStatus::TimedOut,
            ApplicationInvocationStatus::Failed,
        ),
        (
            WorkflowRunStatus::Cancelled,
            ApplicationInvocationStatus::Cancelled,
        ),
    ] {
        let record = terminal_record(workflow_status);
        let projection = application_lifecycle_projection(&record)
            .expect("Application lifecycle projection")
            .expect("terminal projection");
        assert!(projection.final_output.is_none());
        assert_eq!(projection.terminal.status, application_status);
        assert_eq!(
            projection.terminal.completed_at,
            record
                .run
                .finished_at
                .expect("terminal WorkflowRun finish time")
        );
    }
}

fn terminal_record(status: WorkflowRunStatus) -> WorkflowRunRecord {
    let input = application_workflow_run_input().expect("Application WorkflowRun input");
    let actor = PrincipalId::new();
    let (mut run, steps) = WorkflowRun::create(input, actor).expect("WorkflowRun");
    let finished_at = if status == WorkflowRunStatus::TimedOut {
        run.execution_input.deadline_at
    } else {
        run.requested_at + Duration::seconds(2)
    };
    if status == WorkflowRunStatus::Cancelled {
        run.request_cancellation(
            Some("cancelled by test".into()),
            actor,
            run.requested_at + Duration::seconds(1),
        )
        .expect("request cancellation");
    }
    run.project_flow(WorkflowRunFlowState {
        status,
        flow_runtime_build_id: "a3s-cloud-application-lifecycle-test@1".into(),
        last_flow_sequence: 1,
        output: None,
        error: matches!(
            status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut
        )
        .then(|| "terminal failure".into()),
        started_at: Some(run.requested_at),
        finished_at: Some(finished_at),
        observed_at: finished_at,
    })
    .expect("project terminal Flow state");
    let record = WorkflowRunRecord { run, steps };
    record.validate().expect("valid terminal record");
    record
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}
