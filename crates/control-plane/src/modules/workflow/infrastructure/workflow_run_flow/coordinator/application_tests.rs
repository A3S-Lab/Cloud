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
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, EnvironmentId, IdempotentWrite, OntologyId, OntologyRevisionId,
    PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowRun, WorkflowRunFlowState, WorkflowRunRecord,
    WorkflowRunStatus, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_FLOW_VERSION_V10,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{application_workflow_run_input, workflow_run_input};
use a3s_flow::{FlowEngine, RuntimeBuildCompatibility, RuntimeBuildId, WorkflowSpec};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedApplicationEffect {
    FinalOutput(WorkflowApplicationMessageRequest),
    Terminal(WorkflowApplicationTerminalRequest),
}

struct RecordingApplicationEffects {
    inner: WorkflowApplicationEffectsService,
    calls: Mutex<Vec<RecordedApplicationEffect>>,
}

impl RecordingApplicationEffects {
    fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            calls: Mutex::new(Vec::new()),
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
        self.inner.read_conversation_variables(reference).await
    }

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.inner.append_answer(request).await
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
        self.inner.advance_conversation_variables(request).await
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
