use super::{
    ApplicationAccess, ApplicationAccessScope, ObserveApplicationStreamingInvocation,
    ObserveApplicationStreamingInvocationHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AppendApplicationMessageWrite, ApplicationAudience,
    ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
    ApplicationInteractionMode, ApplicationInvocation, ApplicationInvocationStatus,
    ApplicationInvocationWorkflowAuthority, ApplicationMessage, ApplicationMessageKind,
    ApplicationRelease, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, ApplicationSession, ApplicationStreamingObservationStatus,
    ApplicationWorkflowBinding, ApplicationWorkflowEffect, ConversationVariableRevision,
    IApplicationSessionRepository, OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::InMemoryApplicationSessionRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;

struct Fixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    session: ApplicationSession,
    invocation: ApplicationInvocation,
    input: ApplicationMessage,
    answer: ApplicationMessage,
    actor: PrincipalId,
    access: ApplicationAccess,
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
        .with_ymd_and_hms(2026, 9, 14, 21, 0, 0)
        .single()
        .expect("timestamp");
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
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
    let end_user = ApplicationEndUser::project_member(&release, actor, created_at)
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
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
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
    let input = ApplicationMessage::input(
        &session,
        &invocation,
        invocation.requested_at,
    )
    .expect("input");
    sessions
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: invocation.clone(),
            workflow_authority,
            input_message: input.clone(),
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
        json!({"text": "partial"}),
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
    let stored_invocation = sessions
        .find_invocation(
            session.organization_id,
            session.project_id,
            session.application_id,
            running.id,
        )
        .await
        .expect("find invocation")
        .expect("invocation");
    Fixture {
        sessions,
        session: stored,
        invocation: stored_invocation,
        input,
        answer,
        actor,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn observes_open_streaming_invocation() {
    let fixture = fixture().await;
    let observation = ObserveApplicationStreamingInvocationHandler::new(fixture.sessions.clone())
        .execute(
            ObserveApplicationStreamingInvocation {
                organization_id: fixture.session.organization_id,
                project_id: fixture.session.project_id,
                application_id: fixture.session.application_id,
                session_id: fixture.session.id,
                invocation_id: fixture.invocation.id,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                after_sequence: 0,
                observed_at: fixture.answer.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("observe");
    assert_eq!(
        observation.stream_status,
        ApplicationStreamingObservationStatus::Open
    );
    assert_eq!(observation.frames.len(), 2);
    assert_eq!(observation.frames[0].message_id, fixture.input.id);
    assert_eq!(observation.frames[1].message_id, fixture.answer.id);
    assert_eq!(observation.after_sequence, 0);
    assert_eq!(observation.next_sequence, fixture.answer.sequence);
    assert!(!observation.has_more);
    assert_eq!(
        observation.invocation_status,
        ApplicationInvocationStatus::Running
    );
}

#[tokio::test]
async fn observes_streaming_invocation_after_cursor() {
    let fixture = fixture().await;
    let observation = ObserveApplicationStreamingInvocationHandler::new(fixture.sessions.clone())
        .execute(
            ObserveApplicationStreamingInvocation {
                organization_id: fixture.session.organization_id,
                project_id: fixture.session.project_id,
                application_id: fixture.session.application_id,
                session_id: fixture.session.id,
                invocation_id: fixture.invocation.id,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                after_sequence: fixture.input.sequence,
                observed_at: fixture.answer.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("observe");
    assert_eq!(observation.frames.len(), 1);
    assert_eq!(observation.frames[0].message_id, fixture.answer.id);
    assert_eq!(observation.after_sequence, fixture.input.sequence);
    assert_eq!(observation.next_sequence, fixture.answer.sequence);
    assert!(!observation.has_more);
}

#[tokio::test]
async fn unauthorized_project_fails_closed_for_streaming_observation() {
    let fixture = fixture().await;
    let error = ObserveApplicationStreamingInvocationHandler::new(fixture.sessions.clone())
        .execute(
            ObserveApplicationStreamingInvocation {
                organization_id: fixture.session.organization_id,
                project_id: fixture.session.project_id,
                application_id: fixture.session.application_id,
                session_id: fixture.session.id,
                invocation_id: fixture.invocation.id,
                actor_principal_id: fixture.actor,
                access: ApplicationAccess::restricted([ApplicationAccessScope::Project {
                    project_id: ProjectId::new(),
                }]),
                after_sequence: 0,
                observed_at: fixture.answer.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect_err("unauthorized");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn missing_invocation_fails_closed_for_streaming_observation() {
    let fixture = fixture().await;
    let error = ObserveApplicationStreamingInvocationHandler::new(fixture.sessions.clone())
        .execute(
            ObserveApplicationStreamingInvocation {
                organization_id: fixture.session.organization_id,
                project_id: fixture.session.project_id,
                application_id: fixture.session.application_id,
                session_id: fixture.session.id,
                invocation_id: ApplicationInvocationId::new(),
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                after_sequence: 0,
                observed_at: fixture.answer.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect_err("missing");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}
