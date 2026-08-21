use super::{
    IWorkflowApplicationEffectsPort, WorkflowApplicationEffectRequest,
    WorkflowApplicationEffectsService, WorkflowApplicationMessageRequest,
    WorkflowApplicationRunReference, WorkflowApplicationTerminalRequest,
    WorkflowApplicationVariableSnapshot, WorkflowApplicationVariableWriteRequest,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, ApplicationAudience, ApplicationDeliveryPolicy,
    ApplicationEndUser, ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationMessageKind, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
    ApplicationWorkflowBinding, CloseApplicationSessionWrite, ConversationVariableRevision,
    IApplicationSessionRepository, OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::InMemoryApplicationSessionRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, ConversationVariableRevisionId, EnvironmentId,
    IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Fixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    service: WorkflowApplicationEffectsService,
    organization_id: OrganizationId,
    workflow_run_id: WorkflowRunId,
    session_id: ApplicationSessionId,
    invocation_id: ApplicationInvocationId,
    created_at: DateTime<Utc>,
}

impl Fixture {
    fn reference(&self) -> WorkflowApplicationRunReference {
        WorkflowApplicationRunReference {
            organization_id: self.organization_id,
            workflow_run_id: self.workflow_run_id,
        }
    }

    fn effect(
        &self,
        step_id: &str,
        ordinal: u32,
        seconds: i64,
    ) -> WorkflowApplicationEffectRequest {
        WorkflowApplicationEffectRequest {
            organization_id: self.organization_id,
            workflow_run_id: self.workflow_run_id,
            step_id: step_id.into(),
            step_attempt: 1,
            effect_ordinal: ordinal,
            occurred_at: self.created_at + Duration::seconds(seconds),
        }
    }

    fn message(
        &self,
        step_id: &str,
        ordinal: u32,
        seconds: i64,
        content: serde_json::Value,
    ) -> WorkflowApplicationMessageRequest {
        WorkflowApplicationMessageRequest {
            effect: self.effect(step_id, ordinal, seconds),
            content,
        }
    }
}

async fn fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 21, 11, 0, 0)
        .single()
        .expect("timestamp");
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
        created_at,
    )
    .expect("release");
    let end_user = ApplicationEndUser::project_member(&release, actor, created_at)
        .expect("project member end user");
    let session_id = ApplicationSessionId::new();
    let opened_at = created_at + Duration::seconds(1);
    let initial_variables = ConversationVariableRevision::initial(
        session_id,
        &release,
        json!({"locale": "en-US"}),
        opened_at,
    )
    .expect("initial variables");
    let session = ApplicationSession::create(
        session_id,
        &release,
        &end_user,
        &initial_variables,
        opened_at,
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

    let invocation_id = ApplicationInvocationId::new();
    let requested_at = created_at + Duration::seconds(2);
    let invocation = ApplicationInvocation::request(
        invocation_id,
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
    let workflow_run_id = WorkflowRunId::new();
    let running = invocation
        .bind_workflow_run(
            invocation.aggregate_version,
            workflow_run_id,
            created_at + Duration::seconds(3),
        )
        .expect("bind WorkflowRun");
    sessions
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: running,
            expected_version: invocation.aggregate_version,
        })
        .await
        .expect("persist WorkflowRun binding");
    let service = WorkflowApplicationEffectsService::new(sessions.clone());
    Fixture {
        sessions,
        service,
        organization_id,
        workflow_run_id,
        session_id,
        invocation_id,
        created_at,
    }
}

#[tokio::test]
async fn run_only_reference_resolves_exact_application_variable_authority() {
    let fixture = fixture().await;
    let snapshot = fixture
        .service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("variable snapshot");
    snapshot.validate().expect("valid snapshot");
    assert_eq!(snapshot.session_id, fixture.session_id);
    assert_eq!(snapshot.invocation_id, fixture.invocation_id);
    assert_eq!(snapshot.workflow_run_id, fixture.workflow_run_id);
    assert_eq!(snapshot.version.revision_number, 1);
    assert_eq!(snapshot.values, json!({"locale": "en-US"}));

    let foreign = WorkflowApplicationRunReference {
        organization_id: OrganizationId::new(),
        workflow_run_id: fixture.workflow_run_id,
    };
    assert!(matches!(
        fixture.service.read_conversation_variables(&foreign).await,
        Err(ApplicationError::NotFound(_))
    ));
    let missing = WorkflowApplicationRunReference {
        organization_id: fixture.organization_id,
        workflow_run_id: WorkflowRunId::new(),
    };
    assert!(matches!(
        fixture.service.read_conversation_variables(&missing).await,
        Err(ApplicationError::NotFound(_))
    ));

    let mut invalid = fixture.message("bad step", 0, 4, json!({"text": "hidden"}));
    invalid.effect.workflow_run_id = WorkflowRunId::new();
    assert!(matches!(
        fixture.service.append_answer(&invalid).await,
        Err(ApplicationError::Invalid(_))
    ));
}

#[tokio::test]
async fn answer_and_variable_effects_replay_after_later_session_advances() {
    let fixture = fixture().await;
    let answer = fixture.message("answer", 0, 4, json!({"text": "Hello"}));
    let first = fixture
        .service
        .append_answer(&answer)
        .await
        .expect("append Answer");
    assert!(!first.replayed);
    assert_eq!(first.value.kind, ApplicationMessageKind::Answer);
    let replay = fixture
        .service
        .append_answer(&answer)
        .await
        .expect("replay Answer");
    assert!(replay.replayed);
    assert_eq!(replay.value, first.value);

    let mut drifted_answer = answer.clone();
    drifted_answer.content = json!({"text": "Changed"});
    assert!(matches!(
        fixture.service.append_answer(&drifted_answer).await,
        Err(ApplicationError::Conflict(_))
    ));

    let initial = fixture
        .service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("initial variable snapshot");
    let variables = WorkflowApplicationVariableWriteRequest {
        effect: fixture.effect("assign", 0, 5),
        expected: initial.version.clone(),
        values: json!({"locale": "en-US", "topic": "billing"}),
    };
    let advanced = fixture
        .service
        .advance_conversation_variables(&variables)
        .await
        .expect("advance variables");
    assert!(!advanced.replayed);

    let later_answer = fixture.message("answer", 1, 6, json!({"text": "Anything else?"}));
    fixture
        .service
        .append_answer(&later_answer)
        .await
        .expect("later Answer");
    let replay = fixture
        .service
        .advance_conversation_variables(&variables)
        .await
        .expect("variable replay after later message");
    assert!(replay.replayed);
    assert_eq!(replay.value, advanced.value);

    let stale = WorkflowApplicationVariableWriteRequest {
        effect: fixture.effect("assign", 1, 7),
        expected: initial.version,
        values: json!({"locale": "zh-CN"}),
    };
    assert!(matches!(
        fixture.service.advance_conversation_variables(&stale).await,
        Err(ApplicationError::Conflict(_))
    ));

    let current = fixture
        .service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("current variable snapshot");
    assert_eq!(current.version.revision_number, 2);
    assert_eq!(current.values, variables.values);
}

#[tokio::test]
async fn concurrent_redelivery_claims_each_semantic_effect_once() {
    let fixture = fixture().await;
    let request = fixture.message("answer", 0, 4, json!({"text": "Hello"}));
    let (left, right) = tokio::join!(
        fixture.service.append_answer(&request),
        fixture.service.append_answer(&request)
    );
    let left = left.expect("left redelivery");
    let right = right.expect("right redelivery");
    assert_eq!(left.value, right.value);
    assert_ne!(left.replayed, right.replayed);

    let snapshot = fixture
        .service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("variable snapshot");
    let request = WorkflowApplicationVariableWriteRequest {
        effect: fixture.effect("assign", 0, 5),
        expected: snapshot.version,
        values: json!({"locale": "en-US", "topic": "billing"}),
    };
    let (left, right) = tokio::join!(
        fixture.service.advance_conversation_variables(&request),
        fixture.service.advance_conversation_variables(&request)
    );
    let left = left.expect("left variable redelivery");
    let right = right.expect("right variable redelivery");
    assert_eq!(left.value, right.value);
    assert_ne!(left.replayed, right.replayed);
}

#[tokio::test]
async fn final_output_fences_late_frames_and_terminal_observation_replays_exactly() {
    let fixture = fixture().await;
    fixture
        .service
        .append_answer(&fixture.message("answer", 0, 4, json!({"text": "Hello"})))
        .await
        .expect("Answer");
    let final_output = fixture.message("output", 0, 5, json!({"result": "Hello"}));
    let first = fixture
        .service
        .append_final_output(&final_output)
        .await
        .expect("final output");
    assert!(!first.replayed);
    assert_eq!(first.value.kind, ApplicationMessageKind::FinalOutput);
    assert!(
        fixture
            .service
            .append_final_output(&final_output)
            .await
            .expect("final output replay")
            .replayed
    );

    assert!(matches!(
        fixture
            .service
            .append_final_output(&fixture.message("output", 1, 6, json!({"result": "duplicate"}),))
            .await,
        Err(ApplicationError::Conflict(_))
    ));
    assert!(matches!(
        fixture
            .service
            .append_answer(&fixture.message("answer", 1, 6, json!({"text": "late"}),))
            .await,
        Err(ApplicationError::Conflict(_))
    ));

    let terminal = WorkflowApplicationTerminalRequest {
        organization_id: fixture.organization_id,
        workflow_run_id: fixture.workflow_run_id,
        status: ApplicationInvocationStatus::Succeeded,
        completed_at: fixture.created_at + Duration::seconds(7),
    };
    let observed = fixture
        .service
        .observe_terminal(&terminal)
        .await
        .expect("terminal observation");
    assert!(!observed.replayed);
    assert!(
        fixture
            .service
            .observe_terminal(&terminal)
            .await
            .expect("terminal replay")
            .replayed
    );

    let mut drifted = terminal.clone();
    drifted.status = ApplicationInvocationStatus::Failed;
    assert!(matches!(
        fixture.service.observe_terminal(&drifted).await,
        Err(ApplicationError::Conflict(_))
    ));
    drifted.status = terminal.status;
    drifted.completed_at += Duration::seconds(1);
    assert!(matches!(
        fixture.service.observe_terminal(&drifted).await,
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn cross_kind_effect_reuse_is_rejected_by_the_single_claim_authority() {
    let fixture = fixture().await;
    let shared = fixture.effect("semantic", 0, 4);
    fixture
        .service
        .append_answer(&WorkflowApplicationMessageRequest {
            effect: shared.clone(),
            content: json!({"text": "claimed"}),
        })
        .await
        .expect("claim Answer effect");
    let snapshot = fixture
        .service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("variable snapshot");
    assert!(matches!(
        fixture
            .service
            .advance_conversation_variables(&WorkflowApplicationVariableWriteRequest {
                effect: shared,
                expected: snapshot.version,
                values: json!({"locale": "fr-FR"}),
            })
            .await,
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn ambiguous_commit_responses_recover_messages_variables_and_terminal_state() {
    let fixture = fixture().await;
    let repository = Arc::new(AmbiguousEffectRepository::new(fixture.sessions.clone()));
    let service = WorkflowApplicationEffectsService::new(repository);
    let answer = fixture.message("answer", 0, 4, json!({"text": "Hello"}));
    assert!(
        service
            .append_answer(&answer)
            .await
            .expect("recover Answer commit")
            .replayed
    );

    let snapshot = service
        .read_conversation_variables(&fixture.reference())
        .await
        .expect("variable snapshot");
    let variables = WorkflowApplicationVariableWriteRequest {
        effect: fixture.effect("assign", 0, 5),
        expected: snapshot.version,
        values: json!({"locale": "en-US", "topic": "billing"}),
    };
    assert!(
        service
            .advance_conversation_variables(&variables)
            .await
            .expect("recover variable commit")
            .replayed
    );

    let terminal = WorkflowApplicationTerminalRequest {
        organization_id: fixture.organization_id,
        workflow_run_id: fixture.workflow_run_id,
        status: ApplicationInvocationStatus::Failed,
        completed_at: fixture.created_at + Duration::seconds(6),
    };
    assert!(
        service
            .observe_terminal(&terminal)
            .await
            .expect("recover terminal commit")
            .replayed
    );
}

#[tokio::test]
async fn uncommitted_terminal_failure_preserves_the_write_error_and_can_retry() {
    let fixture = fixture().await;
    let repository = Arc::new(
        AmbiguousEffectRepository::with_uncommitted_terminal_failure(fixture.sessions.clone()),
    );
    let service = WorkflowApplicationEffectsService::new(repository);
    let terminal = WorkflowApplicationTerminalRequest {
        organization_id: fixture.organization_id,
        workflow_run_id: fixture.workflow_run_id,
        status: ApplicationInvocationStatus::Failed,
        completed_at: fixture.created_at + Duration::seconds(6),
    };

    assert!(matches!(
        service.observe_terminal(&terminal).await,
        Err(ApplicationError::Internal(message))
            if message == "fixture rejected the terminal write"
    ));
    let written = service
        .observe_terminal(&terminal)
        .await
        .expect("retry terminal observation");
    assert!(!written.replayed);
    assert_eq!(written.value.status, ApplicationInvocationStatus::Failed);
}

#[test]
fn workflow_effect_boundary_values_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowApplicationRunReference>();
    assert_send_sync::<WorkflowApplicationEffectRequest>();
    assert_send_sync::<WorkflowApplicationMessageRequest>();
    assert_send_sync::<WorkflowApplicationVariableWriteRequest>();
    assert_send_sync::<WorkflowApplicationVariableSnapshot>();
    assert_send_sync::<WorkflowApplicationTerminalRequest>();
    assert_send_sync::<WorkflowApplicationEffectsService>();
}

struct AmbiguousEffectRepository {
    inner: Arc<InMemoryApplicationSessionRepository>,
    fail_message: AtomicBool,
    fail_variables: AtomicBool,
    fail_terminal_before_commit: AtomicBool,
    fail_terminal: AtomicBool,
}

impl AmbiguousEffectRepository {
    fn new(inner: Arc<InMemoryApplicationSessionRepository>) -> Self {
        Self {
            inner,
            fail_message: AtomicBool::new(true),
            fail_variables: AtomicBool::new(true),
            fail_terminal_before_commit: AtomicBool::new(false),
            fail_terminal: AtomicBool::new(true),
        }
    }

    fn with_uncommitted_terminal_failure(inner: Arc<InMemoryApplicationSessionRepository>) -> Self {
        Self {
            inner,
            fail_message: AtomicBool::new(false),
            fail_variables: AtomicBool::new(false),
            fail_terminal_before_commit: AtomicBool::new(true),
            fail_terminal: AtomicBool::new(false),
        }
    }

    fn lost_commit() -> RepositoryError {
        RepositoryError::Storage("fixture lost the commit response".into())
    }
}

#[async_trait]
impl IApplicationSessionRepository for AmbiguousEffectRepository {
    async fn open_session(
        &self,
        write: OpenApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        self.inner.open_session(write).await
    }

    async fn request_invocation(
        &self,
        write: RequestApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        self.inner.request_invocation(write).await
    }

    async fn advance_invocation(
        &self,
        write: AdvanceApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        if self
            .fail_terminal_before_commit
            .swap(false, Ordering::SeqCst)
        {
            return Err(RepositoryError::Storage(
                "fixture rejected the terminal write".into(),
            ));
        }
        let result = self.inner.advance_invocation(write).await?;
        if self.fail_terminal.swap(false, Ordering::SeqCst) {
            return Err(Self::lost_commit());
        }
        Ok(result)
    }

    async fn append_message(
        &self,
        write: AppendApplicationMessageWrite,
    ) -> Result<IdempotentWrite<ApplicationMessage>, RepositoryError> {
        let result = self.inner.append_message(write).await?;
        if self.fail_message.swap(false, Ordering::SeqCst) {
            return Err(Self::lost_commit());
        }
        Ok(result)
    }

    async fn advance_variables(
        &self,
        write: AdvanceConversationVariablesWrite,
    ) -> Result<IdempotentWrite<ConversationVariableRevision>, RepositoryError> {
        let result = self.inner.advance_variables(write).await?;
        if self.fail_variables.swap(false, Ordering::SeqCst) {
            return Err(Self::lost_commit());
        }
        Ok(result)
    }

    async fn close_session(
        &self,
        write: CloseApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        self.inner.close_session(write).await
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

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}
