use super::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest, CancelApplicationInvocation,
    CancelApplicationInvocationHandler, CloseApplicationSession, CloseApplicationSessionHandler,
    GetApplicationInvocation, GetApplicationInvocationHandler, GetApplicationSession,
    GetApplicationSessionHandler, IApplicationWorkflowRunPort, OpenApplicationSession,
    OpenApplicationSessionHandler, ReplayApplicationSession, ReplayApplicationSessionHandler,
    RequestApplicationInvocation, RequestApplicationInvocationHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, Application, ApplicationAudience, ApplicationDeliveryPolicy,
    ApplicationEndUser, ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationRecord, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationReleasePublished, ApplicationResponseMode,
    ApplicationSession, ApplicationSessionStatus, ApplicationWorkflowBinding,
    CloseApplicationSessionWrite, ConversationVariableRevision, CreateApplicationWrite,
    IApplicationRepository, IApplicationSessionRepository, OpenApplicationSessionWrite,
    RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationRepository, InMemoryApplicationSessionRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, ConversationVariableRevisionId, EnvironmentId,
    IdempotencyRequest, IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, ResourceName, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId, WorkflowRunId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct RecordingWorkflowRunPort {
    start_calls: AtomicUsize,
    cancellation_calls: AtomicUsize,
    fail_start: AtomicBool,
    return_cancellation_evidence: AtomicBool,
    adopted: Mutex<Option<ApplicationWorkflowRunRequest>>,
}

impl RecordingWorkflowRunPort {
    fn new() -> Self {
        Self {
            start_calls: AtomicUsize::new(0),
            cancellation_calls: AtomicUsize::new(0),
            fail_start: AtomicBool::new(false),
            return_cancellation_evidence: AtomicBool::new(true),
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
        if self.fail_start.load(Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "fixture WorkflowRun start is unavailable".into(),
            ));
        }
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
        self.cancellation_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .return_cancellation_evidence
            .load(Ordering::SeqCst)
            .then(|| self.evidence(request)))
    }
}

/// Simulates a transport failure after a durable repository commit. Delivery
/// handlers must resolve the committed state instead of issuing a second
/// semantic write.
struct AmbiguousCommitSessionRepository {
    inner: Arc<InMemoryApplicationSessionRepository>,
    race_end_user: AtomicBool,
    fail_open: AtomicBool,
    fail_request: AtomicBool,
    fail_close: AtomicBool,
}

impl AmbiguousCommitSessionRepository {
    fn new(inner: Arc<InMemoryApplicationSessionRepository>) -> Self {
        Self {
            inner,
            race_end_user: AtomicBool::new(false),
            fail_open: AtomicBool::new(true),
            fail_request: AtomicBool::new(true),
            fail_close: AtomicBool::new(true),
        }
    }

    fn with_end_user_race(inner: Arc<InMemoryApplicationSessionRepository>) -> Self {
        Self {
            inner,
            race_end_user: AtomicBool::new(true),
            fail_open: AtomicBool::new(false),
            fail_request: AtomicBool::new(false),
            fail_close: AtomicBool::new(false),
        }
    }

    fn ambiguous_commit() -> RepositoryError {
        RepositoryError::Storage("fixture lost the commit response".into())
    }
}

#[async_trait]
impl IApplicationSessionRepository for AmbiguousCommitSessionRepository {
    async fn open_session(
        &self,
        write: OpenApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        if self.race_end_user.swap(false, Ordering::SeqCst) {
            let actor = write
                .end_user
                .linked_principal_id
                .expect("project-member fixture Principal");
            let raced_at = write.session.created_at + Duration::seconds(1);
            let raced_end_user =
                ApplicationEndUser::project_member(&write.release, actor, raced_at)
                    .expect("raced end user");
            let raced_session_id = ApplicationSessionId::new();
            let raced_variables = ConversationVariableRevision::initial(
                raced_session_id,
                &write.release,
                json!({}),
                raced_at,
            )
            .expect("raced variables");
            let raced_session = ApplicationSession::create(
                raced_session_id,
                &write.release,
                &raced_end_user,
                &raced_variables,
                raced_at,
            )
            .expect("raced session");
            self.inner
                .open_session(OpenApplicationSessionWrite {
                    release: write.release,
                    end_user: raced_end_user,
                    session: raced_session,
                    initial_variables: raced_variables,
                })
                .await?;
            return Err(RepositoryError::Conflict(
                "fixture end-user creation race".into(),
            ));
        }
        let result = self.inner.open_session(write).await?;
        if self.fail_open.swap(false, Ordering::SeqCst) {
            return Err(Self::ambiguous_commit());
        }
        Ok(result)
    }

    async fn request_invocation(
        &self,
        write: RequestApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        let result = self.inner.request_invocation(write).await?;
        if self.fail_request.swap(false, Ordering::SeqCst) {
            return Err(Self::ambiguous_commit());
        }
        Ok(result)
    }

    async fn advance_invocation(
        &self,
        write: AdvanceApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        self.inner.advance_invocation(write).await
    }

    async fn append_message(
        &self,
        write: AppendApplicationMessageWrite,
    ) -> Result<IdempotentWrite<ApplicationMessage>, RepositoryError> {
        self.inner.append_message(write).await
    }

    async fn advance_variables(
        &self,
        write: AdvanceConversationVariablesWrite,
    ) -> Result<IdempotentWrite<ConversationVariableRevision>, RepositoryError> {
        self.inner.advance_variables(write).await
    }

    async fn close_session(
        &self,
        write: CloseApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        let result = self.inner.close_session(write).await?;
        if self.fail_close.swap(false, Ordering::SeqCst) {
            return Err(Self::ambiguous_commit());
        }
        Ok(result)
    }

    async fn find_end_user(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        end_user_id: ApplicationEndUserId,
    ) -> Result<Option<ApplicationEndUser>, RepositoryError> {
        self.inner
            .find_end_user(organization_id, project_id, application_id, end_user_id)
            .await
    }

    async fn find_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Option<ApplicationSession>, RepositoryError> {
        self.inner
            .find_session(organization_id, project_id, application_id, session_id)
            .await
    }

    async fn find_invocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError> {
        self.inner
            .find_invocation(organization_id, project_id, application_id, invocation_id)
            .await
    }

    async fn find_invocation_for_workflow_run(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError> {
        self.inner
            .find_invocation_for_workflow_run(organization_id, workflow_run_id)
            .await
    }

    async fn find_message(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        message_id: ApplicationMessageId,
    ) -> Result<Option<ApplicationMessage>, RepositoryError> {
        self.inner
            .find_message(organization_id, project_id, application_id, message_id)
            .await
    }

    async fn find_invocation_workflow_authority(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocationWorkflowAuthority>, RepositoryError> {
        self.inner
            .find_invocation_workflow_authority(
                organization_id,
                project_id,
                application_id,
                invocation_id,
            )
            .await
    }

    async fn list_messages(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<ApplicationMessage>, RepositoryError> {
        self.inner
            .list_messages(
                organization_id,
                project_id,
                application_id,
                session_id,
                after_sequence,
                limit,
            )
            .await
    }

    async fn find_variable_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        revision_id: ConversationVariableRevisionId,
    ) -> Result<Option<ConversationVariableRevision>, RepositoryError> {
        self.inner
            .find_variable_revision(
                organization_id,
                project_id,
                application_id,
                session_id,
                revision_id,
            )
            .await
    }
}

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    workflows: Arc<RecordingWorkflowRunPort>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    release: ApplicationRelease,
    actor: PrincipalId,
    ontology_id: OntologyId,
    ontology_revision_id: OntologyRevisionId,
    ontology_digest: Sha256Digest,
    environment_id: EnvironmentId,
    created_at: chrono::DateTime<Utc>,
}

impl Fixture {
    fn open(&self, session_id: ApplicationSessionId) -> OpenApplicationSession {
        OpenApplicationSession {
            organization_id: self.organization_id,
            project_id: self.project_id,
            application_id: self.application_id,
            application_release_id: self.release.id,
            session_id,
            initial_variables: json!({"locale": "en-US"}),
            actor_principal_id: self.actor,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            opened_at: self.created_at + Duration::seconds(1),
        }
    }

    fn request(
        &self,
        session_id: ApplicationSessionId,
        invocation_id: ApplicationInvocationId,
        expected_session_version: u64,
    ) -> RequestApplicationInvocation {
        RequestApplicationInvocation {
            organization_id: self.organization_id,
            project_id: self.project_id,
            application_id: self.application_id,
            session_id,
            invocation_id,
            expected_session_version,
            response_mode: ApplicationResponseMode::Streaming,
            input: json!({"query": "hello"}),
            ontology_id: self.ontology_id,
            ontology_revision_id: self.ontology_revision_id,
            ontology_digest: self.ontology_digest.clone(),
            environment_id: Some(self.environment_id),
            timeout_seconds: 3_600,
            actor_principal_id: self.actor,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            requested_at: self.created_at + Duration::seconds(2),
        }
    }
}

async fn delivery_fixture(audience: ApplicationAudience) -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 21, 10, 0, 0)
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
        audience,
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
        ResourceName::parse("Delivery application").expect("Application name"),
        "Authorized delivery commands".into(),
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
                "application-delivery-test",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("Application idempotency"),
        })
        .await
        .expect("persist Application");
    Fixture {
        applications,
        sessions: Arc::new(InMemoryApplicationSessionRepository::new()),
        workflows: Arc::new(RecordingWorkflowRunPort::new()),
        organization_id,
        project_id,
        application_id,
        release,
        actor,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: digest('1'),
        environment_id: EnvironmentId::new(),
        created_at,
    }
}

#[tokio::test]
async fn session_open_authorizes_before_replay_and_fences_principal_identity() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    let handler =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone());
    let session_id = ApplicationSessionId::new();

    let mut denied = fixture.open(session_id);
    denied.initial_variables = json!("not an object");
    denied.resource_access = ResourceAccessEvaluator::restricted([]);
    let denied = handler
        .execute(denied, context())
        .await
        .expect("command framework")
        .expect_err("hidden project");
    assert!(matches!(denied, ApplicationError::NotFound(_)));

    let first = handler
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open session");
    assert!(!first.replayed);
    assert_eq!(
        first.end_user.id,
        ApplicationEndUser::project_member_id(fixture.application_id, fixture.actor)
            .expect("stable project-member end user")
    );
    assert_eq!(first.session.status, ApplicationSessionStatus::Active);

    let mut replay = fixture.open(session_id);
    replay.opened_at += Duration::minutes(1);
    let replay = handler
        .execute(replay, context())
        .await
        .expect("command framework")
        .expect("replay open");
    assert!(replay.replayed);
    assert_eq!(replay.session, first.session);
    assert_eq!(replay.end_user, first.end_user);

    let mut drifted = fixture.open(session_id);
    drifted.initial_variables = json!({"locale": "zh-CN"});
    assert!(matches!(
        handler
            .execute(drifted, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));

    let mut foreign_actor = fixture.open(session_id);
    foreign_actor.actor_principal_id = PrincipalId::new();
    assert!(matches!(
        handler
            .execute(foreign_actor, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));

    let anonymous = delivery_fixture(ApplicationAudience::Anonymous).await;
    let anonymous_handler = OpenApplicationSessionHandler::new(
        anonymous.applications.clone(),
        anonymous.sessions.clone(),
    );
    assert!(matches!(
        anonymous_handler
            .execute(anonymous.open(ApplicationSessionId::new()), context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn invocation_request_composes_once_and_supports_authorized_cursor_replay() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    let session_id = ApplicationSessionId::new();
    let opened =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone())
            .execute(fixture.open(session_id), context())
            .await
            .expect("command framework")
            .expect("open session");
    let handler = RequestApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    );

    let mut denied_invalid = fixture.request(
        session_id,
        ApplicationInvocationId::new(),
        opened.session.aggregate_version,
    );
    denied_invalid.timeout_seconds = 0;
    denied_invalid.resource_access = ResourceAccessEvaluator::restricted([]);
    assert!(matches!(
        handler
            .execute(denied_invalid, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));

    let mut invalid_timeout = fixture.request(
        session_id,
        ApplicationInvocationId::new(),
        opened.session.aggregate_version,
    );
    invalid_timeout.timeout_seconds = u64::MAX;
    assert!(matches!(
        handler
            .execute(invalid_timeout, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Invalid(_))
    ));

    let invocation_id = ApplicationInvocationId::new();
    let command = fixture.request(session_id, invocation_id, opened.session.aggregate_version);
    let first = handler
        .execute(command.clone(), context())
        .await
        .expect("command framework")
        .expect("request invocation");
    assert!(!first.invocation_replayed);
    assert!(!first.workflow_replayed);
    assert_eq!(
        first.invocation.status,
        ApplicationInvocationStatus::Running
    );
    assert_eq!(
        first.invocation.workflow_run_id,
        Some(first.workflow.workflow_run_id)
    );

    let mut replay = command.clone();
    replay.expected_session_version = u64::MAX;
    replay.requested_at += Duration::minutes(1);
    let replay = handler
        .execute(replay, context())
        .await
        .expect("command framework")
        .expect("replay invocation");
    assert!(replay.invocation_replayed);
    assert!(replay.workflow_replayed);
    assert_eq!(replay.invocation, first.invocation);
    assert_eq!(fixture.workflows.start_calls.load(Ordering::SeqCst), 2);

    let mut drifted = command;
    drifted.input = json!({"query": "different"});
    assert!(matches!(
        handler
            .execute(drifted, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));

    let session = GetApplicationSessionHandler::new(fixture.sessions.clone())
        .execute(
            GetApplicationSession {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                application_id: fixture.application_id,
                session_id,
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get session");
    assert_eq!(session.session.last_message_sequence, 1);
    assert_eq!(session.current_variables.values, json!({"locale": "en-US"}));

    let invocation = GetApplicationInvocationHandler::new(fixture.sessions.clone())
        .execute(
            GetApplicationInvocation {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                application_id: fixture.application_id,
                session_id,
                invocation_id,
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get invocation");
    assert_eq!(invocation, first.invocation);

    let replay_handler = ReplayApplicationSessionHandler::new(fixture.sessions.clone());
    let replay = replay_handler
        .execute(
            ReplayApplicationSession {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                application_id: fixture.application_id,
                session_id,
                after_sequence: 0,
                limit: Some(1),
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("replay session");
    assert_eq!(replay.messages.len(), 1);
    assert_eq!(replay.messages[0].content, json!({"query": "hello"}));
    assert_eq!(replay.next_sequence, 1);
    assert!(!replay.has_more);

    let mut hidden_invalid_cursor = ReplayApplicationSession {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        application_id: fixture.application_id,
        session_id,
        after_sequence: u64::MAX,
        limit: Some(0),
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::restricted([]),
    };
    assert!(matches!(
        replay_handler
            .execute(hidden_invalid_cursor.clone(), context())
            .await
            .expect("query framework"),
        Err(ApplicationError::NotFound(_))
    ));
    hidden_invalid_cursor.resource_access = ResourceAccessEvaluator::organization_wide();
    assert!(matches!(
        replay_handler
            .execute(hidden_invalid_cursor, context())
            .await
            .expect("query framework"),
        Err(ApplicationError::Invalid(_))
    ));
}

#[tokio::test]
async fn cancellation_and_close_replay_without_second_state_authority() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    let session_id = ApplicationSessionId::new();
    let opened =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone())
            .execute(fixture.open(session_id), context())
            .await
            .expect("command framework")
            .expect("open session");
    let requested = RequestApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    )
    .execute(
        fixture.request(
            session_id,
            ApplicationInvocationId::new(),
            opened.session.aggregate_version,
        ),
        context(),
    )
    .await
    .expect("command framework")
    .expect("request invocation");
    let cancel_handler = CancelApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    );
    let cancel = CancelApplicationInvocation {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        application_id: fixture.application_id,
        session_id,
        invocation_id: requested.invocation.id,
        expected_version: requested.invocation.aggregate_version,
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        requested_at: fixture.created_at + Duration::seconds(3),
    };
    let cancelled = cancel_handler
        .execute(cancel.clone(), context())
        .await
        .expect("command framework")
        .expect("request cancellation");
    assert!(!cancelled.replayed);
    assert_eq!(
        cancelled.invocation.status,
        ApplicationInvocationStatus::Cancelling
    );
    assert!(cancelled.workflow.is_some());

    let replayed = cancel_handler
        .execute(cancel.clone(), context())
        .await
        .expect("command framework")
        .expect("replay cancellation");
    assert!(replayed.replayed);
    assert_eq!(replayed.invocation, cancelled.invocation);
    assert_eq!(
        fixture.workflows.cancellation_calls.load(Ordering::SeqCst),
        2
    );
    let mut stale_cancel = cancel;
    stale_cancel.expected_version = stale_cancel.expected_version.saturating_sub(1);
    assert!(matches!(
        cancel_handler
            .execute(stale_cancel, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));

    let active = fixture
        .sessions
        .find_session(
            fixture.organization_id,
            fixture.project_id,
            fixture.application_id,
            session_id,
        )
        .await
        .expect("read session")
        .expect("session");
    let close_handler =
        CloseApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone());
    let close = CloseApplicationSession {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        application_id: fixture.application_id,
        session_id,
        expected_version: active.aggregate_version,
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        closed_at: fixture.created_at + Duration::seconds(4),
    };
    let closed = close_handler
        .execute(close.clone(), context())
        .await
        .expect("command framework")
        .expect("close session");
    assert!(!closed.replayed);
    assert_eq!(closed.session.status, ApplicationSessionStatus::Closed);
    let close_replay = close_handler
        .execute(close, context())
        .await
        .expect("command framework")
        .expect("replay close");
    assert!(close_replay.replayed);
    assert_eq!(close_replay.session, closed.session);

    let late_request = fixture.request(
        session_id,
        ApplicationInvocationId::new(),
        closed.session.aggregate_version,
    );
    let result = RequestApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    )
    .execute(late_request, context())
    .await
    .expect("command framework");
    assert!(matches!(result, Err(ApplicationError::Invalid(_))));
}

#[tokio::test]
async fn failed_start_remains_cancellable_and_unbound_cancellation_terminalizes() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    fixture.workflows.fail_start.store(true, Ordering::SeqCst);
    fixture
        .workflows
        .return_cancellation_evidence
        .store(false, Ordering::SeqCst);
    let session_id = ApplicationSessionId::new();
    let opened =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone())
            .execute(fixture.open(session_id), context())
            .await
            .expect("command framework")
            .expect("open session");
    let invocation_id = ApplicationInvocationId::new();
    let request = fixture.request(session_id, invocation_id, opened.session.aggregate_version);
    let result = RequestApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    )
    .execute(request, context())
    .await
    .expect("command framework");
    assert!(matches!(result, Err(ApplicationError::Unavailable(_))));
    let admitted = fixture
        .sessions
        .find_invocation(
            fixture.organization_id,
            fixture.project_id,
            fixture.application_id,
            invocation_id,
        )
        .await
        .expect("read invocation")
        .expect("admitted invocation");
    assert_eq!(admitted.status, ApplicationInvocationStatus::Requested);
    assert!(admitted.workflow_run_id.is_none());

    let cancel_handler = CancelApplicationInvocationHandler::new(
        fixture.applications.clone(),
        fixture.sessions.clone(),
        fixture.workflows.clone(),
    );
    let cancel = CancelApplicationInvocation {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        application_id: fixture.application_id,
        session_id,
        invocation_id,
        expected_version: admitted.aggregate_version,
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        requested_at: fixture.created_at + Duration::seconds(3),
    };
    let terminal = cancel_handler
        .execute(cancel.clone(), context())
        .await
        .expect("command framework")
        .expect("cancel unbound invocation");
    assert!(!terminal.replayed);
    assert!(terminal.workflow.is_none());
    assert_eq!(
        terminal.invocation.status,
        ApplicationInvocationStatus::Cancelled
    );

    let replay = cancel_handler
        .execute(cancel, context())
        .await
        .expect("command framework")
        .expect("replay unbound cancellation");
    assert!(replay.replayed);
    assert_eq!(replay.invocation, terminal.invocation);
    assert_eq!(fixture.workflows.start_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.workflows.cancellation_calls.load(Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn ambiguous_repository_commits_are_resolved_as_exact_replays() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    let repository = Arc::new(AmbiguousCommitSessionRepository::new(
        fixture.sessions.clone(),
    ));
    let session_id = ApplicationSessionId::new();
    let opened =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), repository.clone())
            .execute(fixture.open(session_id), context())
            .await
            .expect("command framework")
            .expect("resolve ambiguous open");
    assert!(opened.replayed);

    let invocation_id = ApplicationInvocationId::new();
    let requested = RequestApplicationInvocationHandler::new(
        fixture.applications.clone(),
        repository.clone(),
        fixture.workflows.clone(),
    )
    .execute(
        fixture.request(session_id, invocation_id, opened.session.aggregate_version),
        context(),
    )
    .await
    .expect("command framework")
    .expect("resolve ambiguous invocation request");
    assert!(requested.invocation_replayed);
    assert_eq!(
        requested.invocation.status,
        ApplicationInvocationStatus::Running
    );

    let current = repository
        .find_session(
            fixture.organization_id,
            fixture.project_id,
            fixture.application_id,
            session_id,
        )
        .await
        .expect("read session")
        .expect("session");
    let closed = CloseApplicationSessionHandler::new(fixture.applications.clone(), repository)
        .execute(
            CloseApplicationSession {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                application_id: fixture.application_id,
                session_id,
                expected_version: current.aggregate_version,
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                closed_at: fixture.created_at + Duration::seconds(3),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("resolve ambiguous close");
    assert!(closed.replayed);
    assert_eq!(closed.session.status, ApplicationSessionStatus::Closed);
}

#[tokio::test]
async fn concurrent_session_open_adopts_the_existing_deterministic_end_user() {
    let fixture = delivery_fixture(ApplicationAudience::ProjectMembers).await;
    let repository = Arc::new(AmbiguousCommitSessionRepository::with_end_user_race(
        fixture.sessions.clone(),
    ));
    let session_id = ApplicationSessionId::new();
    let opened = OpenApplicationSessionHandler::new(fixture.applications.clone(), repository)
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("adopt raced end user");

    assert!(!opened.replayed);
    assert_eq!(opened.session.id, session_id);
    assert_eq!(
        opened.end_user.id,
        ApplicationEndUser::project_member_id(fixture.application_id, fixture.actor)
            .expect("deterministic end user")
    );
    assert_eq!(opened.session.created_at, opened.end_user.created_at);
    assert_eq!(
        opened.initial_variables.created_at,
        opened.session.created_at
    );
}

#[test]
fn application_delivery_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<OpenApplicationSession>();
    assert_send_sync::<OpenApplicationSessionHandler>();
    assert_send_sync::<RequestApplicationInvocation>();
    assert_send_sync::<RequestApplicationInvocationHandler>();
    assert_send_sync::<CancelApplicationInvocation>();
    assert_send_sync::<CancelApplicationInvocationHandler>();
    assert_send_sync::<CloseApplicationSession>();
    assert_send_sync::<CloseApplicationSessionHandler>();
    assert_send_sync::<GetApplicationSession>();
    assert_send_sync::<GetApplicationSessionHandler>();
    assert_send_sync::<GetApplicationInvocation>();
    assert_send_sync::<GetApplicationInvocationHandler>();
    assert_send_sync::<ReplayApplicationSession>();
    assert_send_sync::<ReplayApplicationSessionHandler>();
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
