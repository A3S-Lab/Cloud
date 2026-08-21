use super::session_tests::{make_release, session_state};
use super::*;
use crate::modules::applications::infrastructure::InMemoryApplicationSessionRepository;
use crate::modules::shared_kernel::domain::{
    ApplicationInvocationId, OntologyId, OntologyRevisionId, Sha256Digest,
};
use chrono::Duration;
use serde_json::json;

#[tokio::test]
async fn admission_replays_ignore_server_clock_and_sequence_races() {
    let repository = InMemoryApplicationSessionRepository::new();
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (end_user, session, variables) = session_state(&release);
    repository
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user: end_user.clone(),
            session: session.clone(),
            initial_variables: variables.clone(),
        })
        .await
        .expect("open Application session");

    let later = release.created_at + Duration::seconds(30);
    let replay_end_user = ApplicationEndUser::create(
        end_user.id,
        &release,
        end_user.linked_principal_id,
        end_user.created_by,
        later,
    )
    .expect("clock-shifted end user");
    let replay_variables = ConversationVariableRevision::initial(
        session.id,
        &release,
        variables.values.clone(),
        later,
    )
    .expect("clock-shifted variables");
    let replay_session = ApplicationSession::create(
        session.id,
        &release,
        &replay_end_user,
        &replay_variables,
        later,
    )
    .expect("clock-shifted session");
    assert!(
        repository
            .open_session(OpenApplicationSessionWrite {
                release: release.clone(),
                end_user: replay_end_user,
                session: replay_session,
                initial_variables: replay_variables,
            })
            .await
            .expect("clock-shifted open replay")
            .replayed
    );

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        release.created_at + Duration::seconds(1),
    )
    .expect("invocation");
    let authority = ApplicationInvocationWorkflowAuthority::new(
        &invocation,
        OntologyId::new(),
        OntologyRevisionId::new(),
        Sha256Digest::parse(format!("sha256:{}", "8".repeat(64))).expect("Ontology digest"),
        None,
        end_user.created_by,
        300,
    )
    .expect("Workflow authority");
    let input = ApplicationMessage::input(&session, &invocation, invocation.requested_at)
        .expect("input message");
    repository
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: invocation.clone(),
            workflow_authority: authority.clone(),
            input_message: input,
            expected_session_version: session.aggregate_version,
        })
        .await
        .expect("request invocation");

    let stored_session = repository
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("read session")
        .expect("stored session");
    let replay_invocation = ApplicationInvocation::request(
        invocation.id,
        &stored_session,
        &release,
        invocation.response_mode,
        invocation.input.clone(),
        later,
    )
    .expect("clock-shifted invocation");
    let replay_authority = ApplicationInvocationWorkflowAuthority::new(
        &replay_invocation,
        authority.ontology_id,
        authority.ontology_revision_id,
        authority.ontology_digest,
        authority.environment_id,
        authority.requested_by,
        authority.timeout_seconds,
    )
    .expect("clock-shifted authority");
    let replay_input = ApplicationMessage::input(
        &stored_session,
        &replay_invocation,
        replay_invocation.requested_at,
    )
    .expect("clock-shifted input");
    assert!(
        repository
            .request_invocation(RequestApplicationInvocationWrite {
                invocation: replay_invocation,
                workflow_authority: replay_authority,
                input_message: replay_input,
                expected_session_version: stored_session.aggregate_version,
            })
            .await
            .expect("clock-shifted invocation replay")
            .replayed
    );
}
