use super::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest,
    ComposeApplicationInvocationWorkflowRun, ComposeApplicationInvocationWorkflowRunHandler,
    IApplicationWorkflowRunPort,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, Application, ApplicationAudience, ApplicationDeliveryPolicy,
    ApplicationEndUser, ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationMessage, ApplicationRecord, ApplicationRelease,
    ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationReleasePublished,
    ApplicationResponseMode, ApplicationSession, ApplicationWorkflowBinding,
    ConversationVariableRevision, CreateApplicationWrite, IApplicationRepository,
    IApplicationSessionRepository, OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationRepository, InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, EnvironmentId, IdempotencyRequest, OntologyId, OntologyRevisionId,
    OrganizationId, PrincipalId, ProjectId, ResourceName, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId, WorkflowRunId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct ExactWorkflowRunPort {
    calls: AtomicUsize,
    cancellation_calls: AtomicUsize,
    drift_evidence: AtomicBool,
    adopted: Mutex<Option<ApplicationWorkflowRunRequest>>,
    cancel_during_start: Mutex<Option<CancellationRace>>,
}

struct CancellationRace {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    invocation_id: ApplicationInvocationId,
}

impl ExactWorkflowRunPort {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            cancellation_calls: AtomicUsize::new(0),
            drift_evidence: AtomicBool::new(false),
            adopted: Mutex::new(None),
            cancel_during_start: Mutex::new(None),
        }
    }

    fn evidence(&self, request: &ApplicationWorkflowRunRequest) -> ApplicationWorkflowRunEvidence {
        let timeout = Duration::seconds(
            i64::try_from(request.timeout_seconds).expect("fixture timeout fits i64"),
        );
        let mut evidence = ApplicationWorkflowRunEvidence {
            organization_id: request.organization_id,
            project_id: request.project_id,
            application_id: request.application_id,
            application_release_id: request.application_release_id,
            application_release_digest: request.application_release_digest.clone(),
            session_id: request.session_id,
            invocation_id: request.invocation_id,
            workflow_run_id: request.workflow_run_id(),
            workflow_goal_id: request.workflow_goal_id(),
            plan_revision_id: request.plan_revision_id(),
            plan_digest: digest('9'),
            workflow: request.workflow.clone(),
            ontology_id: request.ontology_id,
            ontology_revision_id: request.ontology_revision_id,
            ontology_digest: request.ontology_digest.clone(),
            environment_id: request.environment_id,
            input_digest: request.input_digest.clone(),
            requested_by: request.requested_by,
            requested_at: request.requested_at,
            deadline_at: request.requested_at + timeout,
        };
        if self.drift_evidence.load(Ordering::SeqCst) {
            evidence.workflow_run_id = WorkflowRunId::new();
        }
        evidence
    }
}

#[async_trait]
impl IApplicationWorkflowRunPort for ExactWorkflowRunPort {
    async fn start_or_adopt(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<ApplicationWorkflowRunEvidence> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        {
            let mut adopted = self.adopted.lock().expect("WorkflowRun fixture lock");
            if adopted.as_ref().is_some_and(|current| current != request) {
                return Err(ApplicationError::Conflict(
                    "Application WorkflowRun request drifted".into(),
                ));
            }
            adopted.get_or_insert_with(|| request.clone());
        }
        let race = self
            .cancel_during_start
            .lock()
            .expect("cancellation fixture lock")
            .take();
        if let Some(race) = race {
            let invocation = race
                .sessions
                .find_invocation(
                    race.organization_id,
                    race.project_id,
                    race.application_id,
                    race.invocation_id,
                )
                .await?
                .ok_or_else(|| {
                    ApplicationError::Internal("cancellation fixture lost invocation".into())
                })?;
            let cancelling = invocation
                .request_cancellation(
                    invocation.aggregate_version,
                    invocation.updated_at + Duration::seconds(1),
                )
                .map_err(ApplicationError::Internal)?;
            race.sessions
                .advance_invocation(AdvanceApplicationInvocationWrite {
                    invocation: cancelling,
                    expected_version: invocation.aggregate_version,
                })
                .await?;
        }
        Ok(self.evidence(request))
    }

    async fn request_cancellation(
        &self,
        request: &ApplicationWorkflowRunRequest,
        _reason: &str,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<ApplicationWorkflowRunEvidence>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.cancellation_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.evidence(request)))
    }
}

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    workflows: Arc<ExactWorkflowRunPort>,
    release: ApplicationRelease,
    session: ApplicationSession,
    invocation: ApplicationInvocation,
    command: ComposeApplicationInvocationWorkflowRun,
}

async fn fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 21, 9, 0, 0)
        .single()
        .expect("timestamp");
    let workflow = ApplicationWorkflowBinding {
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_contract_digest: digest('a'),
        workflow_payload_set_digest: digest('b'),
        workflow_semantic_contract_set_digest: digest('c'),
        input_schema_digest: digest('d'),
        output_schema_digest: digest('e'),
    };
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![ApplicationResponseMode::Streaming],
        },
        workflow,
        presentation_digest: digest('f'),
    })
    .expect("Application release contract");
    let application_id = ApplicationId::new();
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        contract,
        actor,
        created_at,
    )
    .expect("Application release");
    let application = Application::create(
        application_id,
        ResourceName::parse("Composed application").expect("Application name"),
        "Typed WorkflowRun composition".into(),
        &release,
    )
    .expect("Application");
    let applications = Arc::new(InMemoryApplicationRepository::new());
    let record = ApplicationRecord::new(application.clone(), release.clone()).expect("record");
    let request_id = Uuid::now_v7();
    applications
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(&application, &release, request_id)
                .expect("Application event"),
            record,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-workflow-composition",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("Application idempotency"),
        })
        .await
        .expect("persist Application");

    let sessions = Arc::new(InMemoryApplicationSessionRepository::new());
    let end_user = ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &release,
        Some(actor),
        actor,
        created_at,
    )
    .expect("end user");
    let session_id = ApplicationSessionId::new();
    let variables = ConversationVariableRevision::initial(
        session_id,
        &release,
        json!({"locale": "en-US"}),
        created_at,
    )
    .expect("variables");
    let session =
        ApplicationSession::create(session_id, &release, &end_user, &variables, created_at)
            .expect("session");
    sessions
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables: variables,
        })
        .await
        .expect("open session");
    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        created_at + Duration::seconds(1),
    )
    .expect("invocation");
    let input_message = ApplicationMessage::input(&session, &invocation, invocation.requested_at)
        .expect("input message");
    sessions
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: invocation.clone(),
            input_message,
            expected_session_version: session.aggregate_version,
        })
        .await
        .expect("request invocation");

    Fixture {
        applications,
        sessions,
        workflows: Arc::new(ExactWorkflowRunPort::new()),
        release,
        session,
        invocation,
        command: ComposeApplicationInvocationWorkflowRun {
            organization_id,
            project_id,
            application_id,
            session_id,
            invocation_id: invocation.id,
            ontology_id: OntologyId::new(),
            ontology_revision_id: OntologyRevisionId::new(),
            ontology_digest: digest('1'),
            environment_id: Some(EnvironmentId::new()),
            requested_by: actor,
            timeout_seconds: 3_600,
        },
    }
}

#[tokio::test]
async fn deterministic_workflow_identity_is_scoped_to_the_application_aggregate() {
    let fixture = fixture().await;
    let request = ApplicationWorkflowRunRequest::from_invocation(
        &fixture.release,
        &fixture.session,
        &fixture.invocation,
        fixture.command.ontology_id,
        fixture.command.ontology_revision_id,
        fixture.command.ontology_digest.clone(),
        fixture.command.environment_id,
        fixture.command.requested_by,
        fixture.command.timeout_seconds,
    )
    .expect("Application WorkflowRun request");
    let mut other_application = request.clone();
    other_application.application_id = ApplicationId::new();
    other_application
        .validate()
        .expect("same invocation UUID remains valid in another Application scope");

    assert_eq!(request.workflow_run_id(), request.workflow_run_id());
    assert_ne!(
        request.workflow_run_id(),
        other_application.workflow_run_id()
    );
    assert_ne!(
        request.workflow_goal_id(),
        other_application.workflow_goal_id()
    );
    assert_ne!(
        request.plan_revision_id(),
        other_application.plan_revision_id()
    );
}

#[tokio::test]
async fn composition_binds_one_deterministic_workflow_run_and_replays() {
    let fixture = fixture().await;
    let handler = ComposeApplicationInvocationWorkflowRunHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    );
    let first = handler
        .execute(fixture.command.clone(), context())
        .await
        .expect("command framework")
        .expect("compose WorkflowRun");
    assert!(!first.replayed);
    assert_eq!(
        first.invocation.status,
        ApplicationInvocationStatus::Running
    );
    assert_eq!(
        first.invocation.workflow_run_id,
        Some(first.workflow.workflow_run_id)
    );

    let replay = handler
        .execute(fixture.command.clone(), context())
        .await
        .expect("command framework")
        .expect("adopt WorkflowRun");
    assert!(replay.replayed);
    assert_eq!(replay.invocation, first.invocation);
    assert_eq!(replay.workflow, first.workflow);
    assert_eq!(fixture.workflows.calls.load(Ordering::SeqCst), 2);

    let mut drifted = fixture.command;
    drifted.timeout_seconds += 1;
    let conflict = handler
        .execute(drifted, context())
        .await
        .expect("command framework");
    assert!(matches!(conflict, Err(ApplicationError::Conflict(_))));
}

#[tokio::test]
async fn composition_rejects_drifted_workflow_evidence_before_binding() {
    let fixture = fixture().await;
    fixture
        .workflows
        .drift_evidence
        .store(true, Ordering::SeqCst);
    let handler = ComposeApplicationInvocationWorkflowRunHandler::new(
        fixture.applications,
        fixture.sessions.clone(),
        fixture.workflows,
    );
    let result = handler
        .execute(fixture.command.clone(), context())
        .await
        .expect("command framework");
    assert!(matches!(result, Err(ApplicationError::Conflict(_))));
    assert_eq!(
        fixture
            .sessions
            .find_invocation(
                fixture.command.organization_id,
                fixture.command.project_id,
                fixture.command.application_id,
                fixture.command.invocation_id,
            )
            .await
            .expect("read invocation")
            .expect("invocation")
            .status,
        ApplicationInvocationStatus::Requested
    );
}

#[tokio::test]
async fn composition_cancels_the_adopted_workflow_when_invocation_cancellation_wins() {
    let fixture = fixture().await;
    *fixture
        .workflows
        .cancel_during_start
        .lock()
        .expect("cancellation fixture lock") = Some(CancellationRace {
        sessions: fixture.sessions.clone(),
        organization_id: fixture.command.organization_id,
        project_id: fixture.command.project_id,
        application_id: fixture.command.application_id,
        invocation_id: fixture.command.invocation_id,
    });
    let handler = ComposeApplicationInvocationWorkflowRunHandler::new(
        fixture.applications,
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    );

    let result = handler
        .execute(fixture.command.clone(), context())
        .await
        .expect("command framework");
    assert!(matches!(result, Err(ApplicationError::Conflict(_))));
    assert_eq!(
        fixture.workflows.cancellation_calls.load(Ordering::SeqCst),
        1
    );
    let invocation = fixture
        .sessions
        .find_invocation(
            fixture.command.organization_id,
            fixture.command.project_id,
            fixture.command.application_id,
            fixture.command.invocation_id,
        )
        .await
        .expect("read invocation")
        .expect("invocation");
    assert_eq!(invocation.status, ApplicationInvocationStatus::Cancelling);
    assert!(invocation.workflow_run_id.is_none());
}

#[test]
fn application_workflow_composition_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplicationWorkflowRunRequest>();
    assert_send_sync::<ApplicationWorkflowRunEvidence>();
    assert_send_sync::<ComposeApplicationInvocationWorkflowRun>();
    assert_send_sync::<ComposeApplicationInvocationWorkflowRunHandler>();
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
