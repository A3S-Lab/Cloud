//! APP0.2-C55 regression coverage for anonymous Application session close and
//! invocation cancel CQRS.

use super::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest,
    CancelAnonymousApplicationInvocation, CancelAnonymousApplicationInvocationHandler,
    CloseAnonymousApplicationSession, CloseAnonymousApplicationSessionHandler,
    IApplicationWorkflowRunPort, OpenAnonymousApplicationSession,
    OpenAnonymousApplicationSessionHandler, RequestAnonymousApplicationInvocation,
    RequestAnonymousApplicationInvocationHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, Application, ApplicationAudience,
    ApplicationDeliveryCredential, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationInvocationStatus, ApplicationRecord,
    ApplicationRelease, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationReleasePublished, ApplicationResponseMode, ApplicationSessionStatus,
    ApplicationWorkflowBinding, CreateApplicationWrite, IApplicationDeliveryCredentialRepository,
    IApplicationRepository, IApplicationSessionRepository,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationDeliveryCredentialRepository, InMemoryApplicationRepository,
    InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, EnvironmentId, IdempotencyRequest, OntologyId, OntologyRevisionId,
    OrganizationId, PrincipalId, ProjectId, ResourceName, SecretId, SecretVersionReference,
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct RecordingWorkflowRunPort {
    start_calls: AtomicUsize,
    cancel_calls: AtomicUsize,
    adopted: Mutex<Option<ApplicationWorkflowRunRequest>>,
}

impl RecordingWorkflowRunPort {
    fn new() -> Self {
        Self {
            start_calls: AtomicUsize::new(0),
            cancel_calls: AtomicUsize::new(0),
            adopted: Mutex::new(None),
        }
    }

    fn evidence(&self, request: &ApplicationWorkflowRunRequest) -> ApplicationWorkflowRunEvidence {
        ApplicationWorkflowRunEvidence {
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
            deadline_at: request.requested_at
                + Duration::seconds(
                    i64::try_from(request.timeout_seconds).expect("fixture timeout fits i64"),
                ),
        }
    }
}

#[async_trait]
impl IApplicationWorkflowRunPort for RecordingWorkflowRunPort {
    fn admit_timeout_seconds(&self, requested: Option<u64>) -> ApplicationResult<u64> {
        let value = requested.unwrap_or(3_600);
        if value == 0 || i64::try_from(value).is_err() {
            return Err(ApplicationError::Invalid(
                "fixture WorkflowRun timeout is unsupported".into(),
            ));
        }
        Ok(value)
    }

    async fn start_or_adopt(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<ApplicationWorkflowRunEvidence> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        let mut adopted = self.adopted.lock().expect("WorkflowRun fixture lock");
        if adopted.as_ref().is_some_and(|current| current != request) {
            return Err(ApplicationError::Conflict(
                "Application WorkflowRun request drifted".into(),
            ));
        }
        adopted.get_or_insert_with(|| request.clone());
        Ok(self.evidence(request))
    }

    async fn request_cancellation(
        &self,
        request: &ApplicationWorkflowRunRequest,
        _reason: &str,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<ApplicationWorkflowRunEvidence>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.evidence(request)))
    }
}

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    credentials: Arc<InMemoryApplicationDeliveryCredentialRepository>,
    workflows: Arc<RecordingWorkflowRunPort>,
    release: ApplicationRelease,
    credential: ApplicationDeliveryCredential,
    actor: PrincipalId,
    ontology_id: OntologyId,
    ontology_revision_id: OntologyRevisionId,
    ontology_digest: Sha256Digest,
    environment_id: EnvironmentId,
    created_at: chrono::DateTime<Utc>,
}

impl Fixture {
    fn open(&self, session_id: ApplicationSessionId) -> OpenAnonymousApplicationSession {
        OpenAnonymousApplicationSession {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            application_release_id: self.release.id,
            session_id,
            credential_lookup_key: self.credential.lookup_key.clone(),
            initial_variables: json!({"locale": "en-US"}),
            opened_at: self.created_at + Duration::seconds(1),
        }
    }

    fn request(
        &self,
        session_id: ApplicationSessionId,
        invocation_id: ApplicationInvocationId,
        expected_session_version: u64,
    ) -> RequestAnonymousApplicationInvocation {
        RequestAnonymousApplicationInvocation {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            session_id,
            invocation_id,
            expected_session_version,
            credential_lookup_key: self.credential.lookup_key.clone(),
            response_mode: ApplicationResponseMode::Streaming,
            input: json!({"query": "hello"}),
            ontology_id: self.ontology_id,
            ontology_revision_id: self.ontology_revision_id,
            ontology_digest: self.ontology_digest.clone(),
            environment_id: Some(self.environment_id),
            timeout_seconds: 3_600,
            requested_at: self.created_at + Duration::seconds(2),
        }
    }

    fn close(
        &self,
        session_id: ApplicationSessionId,
        expected_version: u64,
    ) -> CloseAnonymousApplicationSession {
        CloseAnonymousApplicationSession {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            session_id,
            expected_version,
            credential_lookup_key: self.credential.lookup_key.clone(),
            closed_at: self.created_at + Duration::seconds(5),
        }
    }

    fn cancel(
        &self,
        session_id: ApplicationSessionId,
        invocation_id: ApplicationInvocationId,
        expected_version: u64,
    ) -> CancelAnonymousApplicationInvocation {
        CancelAnonymousApplicationInvocation {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            session_id,
            invocation_id,
            expected_version,
            credential_lookup_key: self.credential.lookup_key.clone(),
            requested_at: self.created_at + Duration::seconds(6),
        }
    }

    fn open_handler(&self) -> OpenAnonymousApplicationSessionHandler {
        OpenAnonymousApplicationSessionHandler::new(
            self.applications.clone(),
            self.sessions.clone(),
            self.credentials.clone(),
        )
    }

    fn invoke_handler(&self) -> RequestAnonymousApplicationInvocationHandler {
        RequestAnonymousApplicationInvocationHandler::new(
            self.applications.clone(),
            self.sessions.clone(),
            self.credentials.clone(),
            self.workflows.clone(),
        )
    }

    fn close_handler(&self) -> CloseAnonymousApplicationSessionHandler {
        CloseAnonymousApplicationSessionHandler::new(
            self.applications.clone(),
            self.sessions.clone(),
            self.credentials.clone(),
        )
    }

    fn cancel_handler(&self) -> CancelAnonymousApplicationInvocationHandler {
        CancelAnonymousApplicationInvocationHandler::new(
            self.applications.clone(),
            self.sessions.clone(),
            self.credentials.clone(),
            self.workflows.clone(),
        )
    }
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

async fn anonymous_lifecycle_fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 9, 15, 8, 0, 0)
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
        audience: ApplicationAudience::Anonymous,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![ApplicationResponseMode::Streaming],
        },
        workflow,
        presentation_digest: digest('f'),
    })
    .expect("Application release contract");
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
        ResourceName::parse("Anonymous lifecycle application").expect("Application name"),
        "Anonymous lifecycle commands".into(),
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
                "application-anonymous-lifecycle-test",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("Application idempotency"),
        })
        .await
        .expect("persist Application");
    let credential = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::new(),
        &release,
        "public-embed-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        actor,
        created_at,
    )
    .expect("credential");
    let credentials = Arc::new(InMemoryApplicationDeliveryCredentialRepository::default());
    credentials
        .create_delivery_credential(credential.clone())
        .await
        .expect("persist credential");
    Fixture {
        applications,
        sessions: Arc::new(InMemoryApplicationSessionRepository::new()),
        credentials,
        workflows: Arc::new(RecordingWorkflowRunPort::new()),
        release,
        credential,
        actor,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: digest('1'),
        environment_id: EnvironmentId::new(),
        created_at,
    }
}

#[tokio::test]
async fn close_anonymous_application_session_closes_once_and_replays() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let first = fixture
        .close_handler()
        .execute(
            fixture.close(session_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("close anonymous session");
    assert!(!first.replayed);
    assert_eq!(first.session.status, ApplicationSessionStatus::Closed);

    let replay = fixture
        .close_handler()
        .execute(
            fixture.close(session_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("close replay");
    assert!(replay.replayed);
    assert_eq!(replay.session.id, first.session.id);
    assert_eq!(replay.session.status, ApplicationSessionStatus::Closed);
}

#[tokio::test]
async fn close_anonymous_application_session_rejects_foreign_credential_lookup_key() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let foreign = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000255").expect("UUID"),
        ),
        &fixture.release,
        "other-embed-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        fixture.actor,
        fixture.created_at,
    )
    .expect("foreign credential");
    fixture
        .credentials
        .create_delivery_credential(foreign.clone())
        .await
        .expect("persist foreign");
    let mut hijack = fixture.close(session_id, opened.session.aggregate_version);
    hijack.credential_lookup_key = foreign.lookup_key;
    assert!(matches!(
        fixture
            .close_handler()
            .execute(hijack, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));
}

#[tokio::test]
async fn close_anonymous_application_session_survives_later_disable() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let mut disabled = fixture.credential.clone();
    disabled
        .disable(
            disabled.generation,
            fixture.created_at + Duration::seconds(3),
        )
        .expect("disable");
    fixture
        .credentials
        .update_delivery_credential(disabled, fixture.credential.generation)
        .await
        .expect("persist disable");
    let closed = fixture
        .close_handler()
        .execute(
            fixture.close(session_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("close after disable");
    assert_eq!(closed.session.status, ApplicationSessionStatus::Closed);
}

#[tokio::test]
async fn cancel_anonymous_application_invocation_cancels_running_and_calls_workflow_port() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let invocation_id = ApplicationInvocationId::new();
    let requested = fixture
        .invoke_handler()
        .execute(
            fixture.request(session_id, invocation_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("request anonymous invocation");
    let first = fixture
        .cancel_handler()
        .execute(
            fixture.cancel(
                session_id,
                invocation_id,
                requested.invocation.aggregate_version,
            ),
            context(),
        )
        .await
        .expect("command framework")
        .expect("cancel anonymous invocation");
    assert!(!first.replayed);
    assert!(matches!(
        first.invocation.status,
        ApplicationInvocationStatus::Cancelling | ApplicationInvocationStatus::Cancelled
    ));
    assert_eq!(fixture.workflows.cancel_calls.load(Ordering::SeqCst), 1);

    let replay = fixture
        .cancel_handler()
        .execute(
            fixture.cancel(
                session_id,
                invocation_id,
                requested.invocation.aggregate_version,
            ),
            context(),
        )
        .await
        .expect("command framework")
        .expect("cancel replay");
    assert!(replay.replayed);
    assert_eq!(
        fixture.workflows.cancel_calls.load(Ordering::SeqCst),
        2,
        "member cancel path re-enters workflow cancellation on replay; anonymous mirrors that"
    );
}

#[tokio::test]
async fn cancel_anonymous_application_invocation_rejects_terminal_invocation() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let invocation_id = ApplicationInvocationId::new();
    let requested = fixture
        .invoke_handler()
        .execute(
            fixture.request(session_id, invocation_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("request anonymous invocation");

    let mut terminal = requested.invocation.clone();
    terminal = terminal
        .observe_terminal(
            terminal.aggregate_version,
            ApplicationInvocationStatus::Succeeded,
            fixture.created_at + Duration::seconds(4),
        )
        .expect("force terminal");
    fixture
        .sessions
        .advance_invocation(AdvanceApplicationInvocationWrite {
            expected_version: requested.invocation.aggregate_version,
            invocation: terminal,
        })
        .await
        .expect("persist terminal");

    assert!(matches!(
        fixture
            .cancel_handler()
            .execute(
                fixture.cancel(
                    session_id,
                    invocation_id,
                    requested.invocation.aggregate_version
                ),
                context()
            )
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn cancel_anonymous_application_invocation_survives_later_disable() {
    let fixture = anonymous_lifecycle_fixture().await;
    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    let invocation_id = ApplicationInvocationId::new();
    let requested = fixture
        .invoke_handler()
        .execute(
            fixture.request(session_id, invocation_id, opened.session.aggregate_version),
            context(),
        )
        .await
        .expect("command framework")
        .expect("request anonymous invocation");
    let mut disabled = fixture.credential.clone();
    disabled
        .disable(
            disabled.generation,
            fixture.created_at + Duration::seconds(4),
        )
        .expect("disable");
    fixture
        .credentials
        .update_delivery_credential(disabled, fixture.credential.generation)
        .await
        .expect("persist disable");
    let cancelled = fixture
        .cancel_handler()
        .execute(
            fixture.cancel(
                session_id,
                invocation_id,
                requested.invocation.aggregate_version,
            ),
            context(),
        )
        .await
        .expect("command framework")
        .expect("cancel after disable");
    assert!(matches!(
        cancelled.invocation.status,
        ApplicationInvocationStatus::Cancelling | ApplicationInvocationStatus::Cancelled
    ));
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn anonymous_lifecycle_handlers_are_send_sync() {
    assert_send_sync::<CloseAnonymousApplicationSessionHandler>();
    assert_send_sync::<CancelAnonymousApplicationInvocationHandler>();
}
