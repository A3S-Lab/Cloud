use super::{
    ApplicationAccess, ApplicationAccessScope, CreateApplicationAnnotation,
    CreateApplicationAnnotationHandler, CreateApplicationFeedback,
    CreateApplicationFeedbackHandler, ListApplicationAnnotationsBySession,
    ListApplicationAnnotationsBySessionHandler, ListApplicationFeedbackBySession,
    ListApplicationFeedbackBySessionHandler,
};
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
    ApplicationFeedbackRating, ApplicationInteractionMode, ApplicationRelease,
    ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
    ApplicationSession, ApplicationWorkflowBinding, ConversationVariableRevision,
    IApplicationSessionRepository, OpenApplicationSessionWrite,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationAnnotationRepository, InMemoryApplicationFeedbackRepository,
    InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationReleaseId, ApplicationSessionId,
    OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;

struct Fixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    feedbacks: Arc<InMemoryApplicationFeedbackRepository>,
    annotations: Arc<InMemoryApplicationAnnotationRepository>,
    _release: ApplicationRelease,
    session: ApplicationSession,
    actor: PrincipalId,
    access: ApplicationAccess,
}

impl Fixture {
    fn create_feedback(&self) -> CreateApplicationFeedback {
        CreateApplicationFeedback {
            organization_id: self.session.organization_id,
            project_id: self.session.project_id,
            application_id: self.session.application_id,
            session_id: self.session.id,
            source_message_id: None,
            rating: ApplicationFeedbackRating::Positive,
            comment: Some("good".into()),
            actor_principal_id: self.actor,
            access: self.access.clone(),
            created_at: self.session.created_at + Duration::seconds(1),
        }
    }

    fn create_annotation(&self) -> CreateApplicationAnnotation {
        CreateApplicationAnnotation {
            organization_id: self.session.organization_id,
            project_id: self.session.project_id,
            application_id: self.session.application_id,
            session_id: self.session.id,
            source_message_id: None,
            content: json!({"note": "highlight"}),
            actor_principal_id: self.actor,
            access: self.access.clone(),
            created_at: self.session.created_at + Duration::seconds(2),
        }
    }
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
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
    Fixture {
        sessions,
        feedbacks: Arc::new(InMemoryApplicationFeedbackRepository::default()),
        annotations: Arc::new(InMemoryApplicationAnnotationRepository::default()),
        _release: release,
        session,
        actor,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn creates_replays_and_lists_feedback() {
    let fixture = fixture().await;
    let handler =
        CreateApplicationFeedbackHandler::new(fixture.sessions.clone(), fixture.feedbacks.clone());
    let command = fixture.create_feedback();
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
    assert_eq!(replayed.feedback, created.feedback);

    let listed = ListApplicationFeedbackBySessionHandler::new(
        fixture.sessions.clone(),
        fixture.feedbacks.clone(),
    )
    .execute(
        ListApplicationFeedbackBySession {
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
    assert_eq!(listed, vec![created.feedback]);
}

#[tokio::test]
async fn unauthorized_project_fails_closed_for_feedback() {
    let fixture = fixture().await;
    let mut command = fixture.create_feedback();
    command.access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    let error =
        CreateApplicationFeedbackHandler::new(fixture.sessions.clone(), fixture.feedbacks.clone())
            .execute(command, context())
            .await
            .expect("boot")
            .expect_err("unauthorized");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn closed_session_fails_closed_for_feedback() {
    let fixture = fixture().await;
    use crate::modules::applications::domain::CloseApplicationSessionWrite;
    let closed_at = fixture.session.created_at + Duration::seconds(5);
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
    let error =
        CreateApplicationFeedbackHandler::new(fixture.sessions.clone(), fixture.feedbacks.clone())
            .execute(fixture.create_feedback(), context())
            .await
            .expect("boot")
            .expect_err("closed");
    assert!(matches!(error, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn creates_replays_and_lists_annotations() {
    let fixture = fixture().await;
    let handler = CreateApplicationAnnotationHandler::new(
        fixture.sessions.clone(),
        fixture.annotations.clone(),
    );
    let command = fixture.create_annotation();
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
    assert_eq!(replayed.annotation, created.annotation);

    let listed = ListApplicationAnnotationsBySessionHandler::new(
        fixture.sessions.clone(),
        fixture.annotations.clone(),
    )
    .execute(
        ListApplicationAnnotationsBySession {
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
    assert_eq!(listed, vec![created.annotation]);
}
