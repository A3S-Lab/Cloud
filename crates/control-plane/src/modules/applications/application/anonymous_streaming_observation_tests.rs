use super::{
    ObserveAnonymousApplicationStreamingInvocation,
    ObserveAnonymousApplicationStreamingInvocationHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AppendApplicationMessageWrite, ApplicationAudience,
    ApplicationDeliveryCredential, ApplicationDeliveryPolicy, ApplicationEndUser,
    ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationMessageKind, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
    ApplicationStreamingObservationStatus, ApplicationWorkflowBinding, ApplicationWorkflowEffect,
    ConversationVariableRevision, IApplicationDeliveryCredentialRepository,
    IApplicationSessionRepository, OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationDeliveryCredentialRepository, InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    SecretId, SecretVersionReference, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
    WorkflowRunId,
};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    sessions: Arc<InMemoryApplicationSessionRepository>,
    credentials: Arc<InMemoryApplicationDeliveryCredentialRepository>,
    release: ApplicationRelease,
    session: ApplicationSession,
    invocation: ApplicationInvocation,
    input: ApplicationMessage,
    answer: ApplicationMessage,
    credential: ApplicationDeliveryCredential,
    actor: PrincipalId,
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
        .with_ymd_and_hms(2026, 9, 14, 22, 0, 0)
        .single()
        .expect("timestamp");
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::Anonymous,
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
    let credential = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000049").expect("UUID"),
        ),
        &release,
        "anon-streaming-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        actor,
        created_at,
    )
    .expect("credential");
    let end_user_id = credential.anonymous_end_user_id().expect("end user id");
    let end_user =
        ApplicationEndUser::anonymous_credential(end_user_id, &release, actor, created_at)
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
    let credentials = Arc::new(InMemoryApplicationDeliveryCredentialRepository::default());
    credentials
        .create_delivery_credential(credential.clone())
        .await
        .expect("persist credential");
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
    let input =
        ApplicationMessage::input(&session, &invocation, invocation.requested_at).expect("input");
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
        credentials,
        release,
        session: stored,
        invocation: stored_invocation,
        input,
        answer,
        credential,
        actor,
    }
}

fn observe_query(
    fixture: &Fixture,
    after_sequence: u64,
) -> ObserveAnonymousApplicationStreamingInvocation {
    ObserveAnonymousApplicationStreamingInvocation {
        organization_id: fixture.session.organization_id,
        project_id: fixture.session.project_id,
        application_id: fixture.session.application_id,
        session_id: fixture.session.id,
        invocation_id: fixture.invocation.id,
        credential_lookup_key: fixture.credential.lookup_key.clone(),
        after_sequence,
        observed_at: fixture.answer.created_at + Duration::seconds(1),
    }
}

fn handler(fixture: &Fixture) -> ObserveAnonymousApplicationStreamingInvocationHandler {
    ObserveAnonymousApplicationStreamingInvocationHandler::new(
        fixture.sessions.clone(),
        fixture.credentials.clone(),
    )
}

#[tokio::test]
async fn observes_open_anonymous_streaming_invocation() {
    let fixture = fixture().await;
    let observation = handler(&fixture)
        .execute(observe_query(&fixture, 0), context())
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
async fn observes_anonymous_streaming_invocation_after_cursor() {
    let fixture = fixture().await;
    let observation = handler(&fixture)
        .execute(observe_query(&fixture, fixture.input.sequence), context())
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
async fn missing_credential_fails_closed_for_anonymous_streaming_observation() {
    let fixture = fixture().await;
    let mut query = observe_query(&fixture, 0);
    query.credential_lookup_key = "missing-key".into();
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("missing credential");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn foreign_credential_fails_closed_for_anonymous_streaming_observation() {
    let fixture = fixture().await;
    let foreign = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000199").expect("UUID"),
        ),
        &fixture.release,
        "other-stream-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        fixture.actor,
        fixture.answer.created_at,
    )
    .expect("foreign");
    fixture
        .credentials
        .create_delivery_credential(foreign.clone())
        .await
        .expect("persist foreign");
    let mut query = observe_query(&fixture, 0);
    query.credential_lookup_key = foreign.lookup_key;
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("foreign");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn disabled_credential_still_observes_admitted_anonymous_streaming_session() {
    let fixture = fixture().await;
    let mut disabled = fixture.credential.clone();
    disabled
        .disable(
            disabled.generation,
            fixture.answer.created_at + Duration::seconds(2),
        )
        .expect("disable");
    fixture
        .credentials
        .update_delivery_credential(disabled, fixture.credential.generation)
        .await
        .expect("persist disable");
    let observation = handler(&fixture)
        .execute(observe_query(&fixture, 0), context())
        .await
        .expect("boot")
        .expect("observe after disable");
    assert_eq!(
        observation.stream_status,
        ApplicationStreamingObservationStatus::Open
    );
}

#[tokio::test]
async fn missing_invocation_fails_closed_for_anonymous_streaming_observation() {
    let fixture = fixture().await;
    let mut query = observe_query(&fixture, 0);
    query.invocation_id = ApplicationInvocationId::new();
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("missing invocation");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}
