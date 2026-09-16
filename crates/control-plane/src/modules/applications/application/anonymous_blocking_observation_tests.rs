use super::{
    ObserveAnonymousApplicationBlockingInvocation,
    ObserveAnonymousApplicationBlockingInvocationHandler,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AppendApplicationMessageWrite, ApplicationAudience,
    ApplicationBlockingWaitStatus, ApplicationDeliveryCredential, ApplicationDeliveryPolicy,
    ApplicationEndUser, ApplicationExperience, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationMessageKind, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
    ApplicationWorkflowBinding, ApplicationWorkflowEffect, ConversationVariableRevision,
    IApplicationDeliveryCredentialRepository, IApplicationSessionRepository,
    OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
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
        .with_ymd_and_hms(2026, 9, 14, 21, 0, 0)
        .single()
        .expect("timestamp");
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::Anonymous,
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
    let credential = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000048").expect("UUID"),
        ),
        &release,
        "anon-blocking-key",
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
        ApplicationResponseMode::Blocking,
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

fn observe_query(fixture: &Fixture) -> ObserveAnonymousApplicationBlockingInvocation {
    ObserveAnonymousApplicationBlockingInvocation {
        organization_id: fixture.session.organization_id,
        project_id: fixture.session.project_id,
        application_id: fixture.session.application_id,
        session_id: fixture.session.id,
        invocation_id: fixture.invocation.id,
        credential_lookup_key: fixture.credential.lookup_key.clone(),
        observed_at: fixture.answer.created_at + Duration::seconds(1),
    }
}

fn handler(fixture: &Fixture) -> ObserveAnonymousApplicationBlockingInvocationHandler {
    ObserveAnonymousApplicationBlockingInvocationHandler::new(
        fixture.sessions.clone(),
        fixture.credentials.clone(),
    )
}

#[tokio::test]
async fn observes_waiting_anonymous_blocking_invocation() {
    let fixture = fixture().await;
    let observation = handler(&fixture)
        .execute(observe_query(&fixture), context())
        .await
        .expect("boot")
        .expect("observe");
    assert_eq!(
        observation.wait_status,
        ApplicationBlockingWaitStatus::Waiting
    );
    assert_eq!(observation.input_message_id, Some(fixture.input.id));
    assert_eq!(observation.answer_message_ids, vec![fixture.answer.id]);
    assert_eq!(observation.final_output_message_id, None);
    assert_eq!(
        observation.invocation_status,
        ApplicationInvocationStatus::Running
    );
}

#[tokio::test]
async fn observes_succeeded_anonymous_blocking_invocation() {
    let fixture = fixture().await;
    let final_output = ApplicationMessage::workflow_frame(
        &fixture.session,
        &fixture.invocation,
        ApplicationMessageKind::FinalOutput,
        ApplicationWorkflowEffect::new(
            fixture.invocation.workflow_run_id.expect("run"),
            "final",
            1,
            0,
        )
        .expect("effect"),
        json!({"result": "done"}),
        fixture.answer.created_at + Duration::seconds(1),
    )
    .expect("final");
    fixture
        .sessions
        .append_message(AppendApplicationMessageWrite {
            message: final_output.clone(),
            expected_session_version: fixture.session.aggregate_version,
        })
        .await
        .expect("append final");
    let succeeded = fixture
        .invocation
        .observe_terminal(
            fixture.invocation.aggregate_version,
            ApplicationInvocationStatus::Succeeded,
            final_output.created_at + Duration::seconds(1),
        )
        .expect("terminal");
    fixture
        .sessions
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: succeeded.clone(),
            expected_version: fixture.invocation.aggregate_version,
        })
        .await
        .expect("advance");

    let mut query = observe_query(&fixture);
    query.invocation_id = succeeded.id;
    query.observed_at = succeeded.updated_at + Duration::seconds(1);
    let observation = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect("observe");
    assert_eq!(
        observation.wait_status,
        ApplicationBlockingWaitStatus::Succeeded
    );
    assert_eq!(observation.final_output_message_id, Some(final_output.id));
}

#[tokio::test]
async fn missing_credential_fails_closed_for_anonymous_blocking_observation() {
    let fixture = fixture().await;
    let mut query = observe_query(&fixture);
    query.credential_lookup_key = "missing-key".into();
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("missing credential");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn foreign_credential_fails_closed_for_anonymous_blocking_observation() {
    let fixture = fixture().await;
    let foreign = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000099").expect("UUID"),
        ),
        &fixture.release,
        "other-embed-key",
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
    let mut query = observe_query(&fixture);
    query.credential_lookup_key = foreign.lookup_key;
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("foreign");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn disabled_credential_still_observes_admitted_anonymous_session() {
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
        .execute(observe_query(&fixture), context())
        .await
        .expect("boot")
        .expect("observe after disable");
    assert_eq!(
        observation.wait_status,
        ApplicationBlockingWaitStatus::Waiting
    );
}

#[tokio::test]
async fn missing_invocation_fails_closed_for_anonymous_blocking_observation() {
    let fixture = fixture().await;
    let mut query = observe_query(&fixture);
    query.invocation_id = ApplicationInvocationId::new();
    let error = handler(&fixture)
        .execute(query, context())
        .await
        .expect("boot")
        .expect_err("missing invocation");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}
