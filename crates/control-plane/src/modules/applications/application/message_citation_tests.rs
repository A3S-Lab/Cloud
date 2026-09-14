use super::{
    ApplicationAccess, ApplicationAccessScope, CreateApplicationMessageCitation,
    CreateApplicationMessageCitationHandler, GetApplicationMessageCitation,
    GetApplicationMessageCitationHandler, ListApplicationMessageCitationsBySession,
    ListApplicationMessageCitationsBySessionHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AppendApplicationMessageWrite, ApplicationAudience,
    ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
    ApplicationInteractionMode, ApplicationInvocation, ApplicationInvocationWorkflowAuthority,
    ApplicationMessage, ApplicationMessageKind, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
    ApplicationWorkflowBinding, ApplicationWorkflowEffect, CloseApplicationSessionWrite,
    ConversationVariableRevision, IApplicationSessionRepository, OpenApplicationSessionWrite,
    RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationMessageCitationRepository, InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, KnowledgeBaseId, KnowledgeBaseRevisionId,
    KnowledgeChunkId, KnowledgeDocumentId, OntologyId, OntologyRevisionId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;

struct Fixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    citations: Arc<InMemoryApplicationMessageCitationRepository>,
    session: ApplicationSession,
    answer: ApplicationMessage,
    actor: PrincipalId,
    access: ApplicationAccess,
}

impl Fixture {
    fn create_citation(&self) -> CreateApplicationMessageCitation {
        CreateApplicationMessageCitation {
            organization_id: self.session.organization_id,
            project_id: self.session.project_id,
            application_id: self.session.application_id,
            session_id: self.session.id,
            message_id: self.answer.id,
            knowledge_base_id: KnowledgeBaseId::new(),
            knowledge_base_revision_id: KnowledgeBaseRevisionId::new(),
            knowledge_document_id: KnowledgeDocumentId::new(),
            knowledge_chunk_id: KnowledgeChunkId::new(),
            excerpt: Some(" retention is 30 days ".into()),
            actor_principal_id: self.actor,
            access: self.access.clone(),
            created_at: self.answer.created_at + Duration::seconds(1),
        }
    }
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!(
        "sha256:{}",
        format!("{:02x}", marker as u8).repeat(32)
    ))
    .expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

async fn fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 9, 14, 16, 0, 0)
        .single()
        .expect("timestamp");
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![ApplicationResponseMode::Blocking],
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
    .expect("contract");
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        contract,
        actor,
        created_at,
    )
    .expect("release");
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
    let sessions = Arc::new(InMemoryApplicationSessionRepository::default());
    sessions
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables: variables,
        })
        .await
        .expect("open");
    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Blocking,
        json!({"query": "what does the policy say?"}),
        created_at + Duration::seconds(1),
    )
    .expect("invocation");
    let workflow_authority = ApplicationInvocationWorkflowAuthority::new(
        &invocation,
        OntologyId::new(),
        OntologyRevisionId::new(),
        digest('1'),
        None,
        actor,
        3_600,
    )
    .expect("authority");
    sessions
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: invocation.clone(),
            workflow_authority,
            input_message: ApplicationMessage::input(
                &session,
                &invocation,
                invocation.requested_at,
            )
            .expect("input"),
            expected_session_version: 1,
        })
        .await
        .expect("request");
    let run_id = WorkflowRunId::new();
    let running = invocation
        .bind_workflow_run(1, run_id, created_at + Duration::seconds(2))
        .expect("running");
    sessions
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: running.clone(),
            expected_version: 1,
        })
        .await
        .expect("bind");
    let after_input = sessions
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("find session")
        .expect("session");
    let effect = ApplicationWorkflowEffect::new(run_id, "answer", 1, 0).expect("effect");
    let answer = ApplicationMessage::workflow_frame(
        &after_input,
        &running,
        ApplicationMessageKind::Answer,
        effect,
        json!({"text": "Per section 3.2, retention is 30 days."}),
        created_at + Duration::seconds(3),
    )
    .expect("answer");
    sessions
        .append_message(AppendApplicationMessageWrite {
            message: answer.clone(),
            expected_session_version: 2,
        })
        .await
        .expect("append");
    let stored = sessions
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("reload session")
        .expect("session");
    Fixture {
        sessions,
        citations: Arc::new(InMemoryApplicationMessageCitationRepository::default()),
        session: stored,
        answer,
        actor,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn creates_replays_lists_and_gets_message_citations() {
    let fixture = fixture().await;
    let handler = CreateApplicationMessageCitationHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    );
    let command = fixture.create_citation();
    let created = handler
        .execute(command.clone(), context())
        .await
        .expect("boot")
        .expect("create");
    assert!(!created.replayed);
    let replayed = handler
        .execute(command, context())
        .await
        .expect("boot")
        .expect("replay");
    assert!(replayed.replayed);
    assert_eq!(replayed.citation, created.citation);

    let listed = ListApplicationMessageCitationsBySessionHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    )
    .execute(
        ListApplicationMessageCitationsBySession {
            organization_id: fixture.session.organization_id,
            project_id: fixture.session.project_id,
            application_id: fixture.session.application_id,
            session_id: fixture.session.id,
            actor_principal_id: fixture.actor,
            access: fixture.access.clone(),
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("list");
    assert_eq!(listed, vec![created.citation.clone()]);

    let found = GetApplicationMessageCitationHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    )
    .execute(
        GetApplicationMessageCitation {
            organization_id: fixture.session.organization_id,
            project_id: fixture.session.project_id,
            application_id: fixture.session.application_id,
            session_id: fixture.session.id,
            citation_id: created.citation.id,
            actor_principal_id: fixture.actor,
            access: fixture.access.clone(),
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("get");
    assert_eq!(found, created.citation);
}

#[tokio::test]
async fn unauthorized_project_fails_closed_for_message_citations() {
    let fixture = fixture().await;
    let mut command = fixture.create_citation();
    command.access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    let error = CreateApplicationMessageCitationHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    )
    .execute(command, context())
    .await
    .expect("boot")
    .expect_err("unauthorized");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn closed_session_fails_closed_for_message_citations() {
    let fixture = fixture().await;
    let closed_at = fixture.session.created_at + Duration::seconds(8);
    let closed = fixture
        .session
        .close(fixture.session.aggregate_version, closed_at)
        .expect("close domain");
    fixture
        .sessions
        .close_session(CloseApplicationSessionWrite {
            session: closed,
            expected_version: fixture.session.aggregate_version,
        })
        .await
        .expect("close");
    let error = CreateApplicationMessageCitationHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    )
    .execute(fixture.create_citation(), context())
    .await
    .expect("boot")
    .expect_err("closed");
    assert!(matches!(error, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn missing_source_message_fails_closed_for_message_citations() {
    let fixture = fixture().await;
    let mut command = fixture.create_citation();
    command.message_id = ApplicationMessageId::new();
    let error = CreateApplicationMessageCitationHandler::new(
        fixture.sessions.clone(),
        fixture.citations.clone(),
    )
    .execute(command, context())
    .await
    .expect("boot")
    .expect_err("missing source");
    assert!(matches!(error, ApplicationError::Invalid(_)));
}
