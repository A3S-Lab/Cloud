use super::session_tests::{make_release, session_state};
use super::*;
use crate::modules::applications::infrastructure::InMemoryApplicationSessionRepository;
use crate::modules::shared_kernel::domain::{
    ApplicationInvocationId, OntologyId, OntologyRevisionId, PrincipalId, RepositoryError,
    Sha256Digest, WorkflowRunId,
};
use chrono::Duration;
use serde_json::json;

fn workflow_authority(
    invocation: &ApplicationInvocation,
    requested_by: PrincipalId,
) -> ApplicationInvocationWorkflowAuthority {
    ApplicationInvocationWorkflowAuthority::new(
        invocation,
        OntologyId::new(),
        OntologyRevisionId::new(),
        Sha256Digest::parse(format!("sha256:{}", "1".repeat(64))).expect("Ontology digest"),
        None,
        requested_by,
        3_600,
    )
    .expect("invocation Workflow authority")
}

#[tokio::test]
async fn session_and_invocation_writes_are_atomic_and_replay_safe() {
    let repository = InMemoryApplicationSessionRepository::new();
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (end_user, session, initial_variables) = session_state(&release);
    let open = OpenApplicationSessionWrite {
        release: release.clone(),
        end_user: end_user.clone(),
        session: session.clone(),
        initial_variables: initial_variables.clone(),
    };

    let opened = repository
        .open_session(open.clone())
        .await
        .expect("open session");
    assert!(!opened.replayed);
    assert_eq!(opened.value, session);
    let replay = repository.open_session(open).await.expect("open replay");
    assert!(replay.replayed);

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        release.created_at + Duration::seconds(1),
    )
    .expect("invocation");
    let input = ApplicationMessage::input(&session, &invocation, invocation.requested_at)
        .expect("input message");
    let workflow_authority = workflow_authority(&invocation, release.created_by);
    let request = RequestApplicationInvocationWrite {
        invocation: invocation.clone(),
        workflow_authority: workflow_authority.clone(),
        input_message: input.clone(),
        expected_session_version: 1,
    };
    let requested = repository
        .request_invocation(request.clone())
        .await
        .expect("request invocation");
    assert!(!requested.replayed);
    assert_eq!(requested.value, invocation);
    let request_replay = repository
        .request_invocation(request)
        .await
        .expect("request replay");
    assert!(request_replay.replayed);
    assert_eq!(request_replay.value, invocation);
    assert_eq!(
        repository
            .find_invocation_workflow_authority(
                invocation.organization_id,
                invocation.project_id,
                invocation.application_id,
                invocation.id,
            )
            .await
            .expect("find invocation Workflow authority"),
        Some(workflow_authority.clone())
    );

    let stored_session = repository
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("find session")
        .expect("stored session");
    assert_eq!(stored_session.aggregate_version, 2);
    assert_eq!(stored_session.last_message_sequence, 1);
    let late_open_replay = repository
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables,
        })
        .await
        .expect("open replay after session advancement");
    assert!(late_open_replay.replayed);
    assert_eq!(late_open_replay.value, stored_session);
    assert_eq!(
        repository
            .list_messages(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
                0,
                10,
            )
            .await
            .expect("list messages"),
        vec![input]
    );

    let changed_invocation = ApplicationInvocation::request(
        invocation.id,
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "changed"}),
        invocation.requested_at,
    )
    .expect("changed invocation");
    let changed_input =
        ApplicationMessage::input(&session, &changed_invocation, invocation.requested_at)
            .expect("changed input");
    let conflict = repository
        .request_invocation(RequestApplicationInvocationWrite {
            invocation: changed_invocation,
            workflow_authority,
            input_message: changed_input,
            expected_session_version: 1,
        })
        .await;
    assert!(matches!(conflict, Err(RepositoryError::Conflict(_))));
    assert_eq!(
        repository
            .find_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("find session")
            .expect("stored session")
            .aggregate_version,
        2
    );
}

#[tokio::test]
async fn workflow_effects_are_once_only_across_messages_and_variables() {
    let repository = InMemoryApplicationSessionRepository::new();
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (end_user, session, initial_variables) = session_state(&release);
    repository
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables: initial_variables.clone(),
        })
        .await
        .expect("open session");
    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        release.created_at + Duration::seconds(1),
    )
    .expect("invocation");
    let workflow_authority = workflow_authority(&invocation, release.created_by);
    repository
        .request_invocation(RequestApplicationInvocationWrite {
            input_message: ApplicationMessage::input(
                &session,
                &invocation,
                invocation.requested_at,
            )
            .expect("input"),
            invocation: invocation.clone(),
            workflow_authority,
            expected_session_version: 1,
        })
        .await
        .expect("request invocation");

    let run_id = WorkflowRunId::new();
    let running = invocation
        .bind_workflow_run(1, run_id, release.created_at + Duration::seconds(2))
        .expect("running invocation");
    let advanced = repository
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: running.clone(),
            expected_version: 1,
        })
        .await
        .expect("bind WorkflowRun");
    assert!(!advanced.replayed);
    let replay = repository
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: running.clone(),
            expected_version: 1,
        })
        .await
        .expect("bind replay");
    assert!(replay.replayed);

    let after_input = repository
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
        effect.clone(),
        json!({"text": "Hello"}),
        release.created_at + Duration::seconds(3),
    )
    .expect("answer");
    let appended = repository
        .append_message(AppendApplicationMessageWrite {
            message: answer.clone(),
            expected_session_version: 2,
        })
        .await
        .expect("append answer");
    assert!(!appended.replayed);
    let replayed = repository
        .append_message(AppendApplicationMessageWrite {
            message: answer.clone(),
            expected_session_version: 2,
        })
        .await
        .expect("answer replay");
    assert!(replayed.replayed);

    let drifted = ApplicationMessage::workflow_frame(
        &after_input,
        &running,
        ApplicationMessageKind::Answer,
        effect.clone(),
        json!({"text": "Changed"}),
        release.created_at + Duration::seconds(3),
    )
    .expect("drifted answer");
    assert!(matches!(
        repository
            .append_message(AppendApplicationMessageWrite {
                message: drifted,
                expected_session_version: 2,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let final_output = ApplicationMessage::workflow_frame(
        &after_input,
        &running,
        ApplicationMessageKind::FinalOutput,
        effect.clone(),
        json!({"result": "Hello"}),
        release.created_at + Duration::seconds(3),
    )
    .expect("final output");
    assert!(matches!(
        repository
            .append_message(AppendApplicationMessageWrite {
                message: final_output,
                expected_session_version: 2,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let after_answer = repository
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("find session")
        .expect("session");
    let variable_effect =
        ApplicationWorkflowEffect::new(run_id, "assign", 1, 0).expect("variable effect");
    let revision = ConversationVariableRevision::successor(
        &initial_variables,
        variable_effect,
        json!({"locale": "en-US", "topic": "billing"}),
        release.created_at + Duration::seconds(4),
    )
    .expect("variable revision");
    let assigned = repository
        .advance_variables(AdvanceConversationVariablesWrite {
            revision: revision.clone(),
            expected_session_version: after_answer.aggregate_version,
        })
        .await
        .expect("assign variables");
    assert!(!assigned.replayed);
    let replayed = repository
        .advance_variables(AdvanceConversationVariablesWrite {
            revision: revision.clone(),
            expected_session_version: after_answer.aggregate_version,
        })
        .await
        .expect("variable replay");
    assert!(replayed.replayed);

    let effect_collision = ConversationVariableRevision::successor(
        &initial_variables,
        effect,
        json!({"locale": "fr-FR"}),
        release.created_at + Duration::seconds(4),
    )
    .expect("effect collision");
    assert!(matches!(
        repository
            .advance_variables(AdvanceConversationVariablesWrite {
                revision: effect_collision,
                expected_session_version: after_answer.aggregate_version,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let stored = repository
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("find session")
        .expect("session");
    let regressed_answer = ApplicationMessage::workflow_frame(
        &stored,
        &running,
        ApplicationMessageKind::Answer,
        ApplicationWorkflowEffect::new(run_id, "answer", 1, 1).expect("regressed effect"),
        json!({"text": "Out of order"}),
        release.created_at + Duration::seconds(3),
    )
    .expect("well-formed but time-regressing answer");
    assert!(matches!(
        repository
            .append_message(AppendApplicationMessageWrite {
                message: regressed_answer,
                expected_session_version: stored.aggregate_version,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let final_effect =
        ApplicationWorkflowEffect::new(run_id, "output", 1, 0).expect("final effect");
    let final_output = ApplicationMessage::workflow_frame(
        &stored,
        &running,
        ApplicationMessageKind::FinalOutput,
        final_effect,
        json!({"result": "Hello"}),
        release.created_at + Duration::seconds(5),
    )
    .expect("final output");
    repository
        .append_message(AppendApplicationMessageWrite {
            message: final_output,
            expected_session_version: stored.aggregate_version,
        })
        .await
        .expect("append final output");

    let after_final = repository
        .find_session(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
        )
        .await
        .expect("find session")
        .expect("session");
    let late_answer = ApplicationMessage::workflow_frame(
        &after_final,
        &running,
        ApplicationMessageKind::Answer,
        ApplicationWorkflowEffect::new(run_id, "answer", 1, 1).expect("late effect"),
        json!({"text": "Too late"}),
        release.created_at + Duration::seconds(6),
    )
    .expect("late answer");
    assert!(matches!(
        repository
            .append_message(AppendApplicationMessageWrite {
                message: late_answer,
                expected_session_version: after_final.aggregate_version,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(after_final.last_message_sequence, 3);
    assert_eq!(after_final.current_variable_revision_number, 2);
    assert_eq!(after_final.aggregate_version, 5);
}

#[tokio::test]
async fn stale_or_closed_session_writes_leave_no_partial_state() {
    let repository = InMemoryApplicationSessionRepository::new();
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (end_user, session, initial_variables) = session_state(&release);
    repository
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user,
            session: session.clone(),
            initial_variables,
        })
        .await
        .expect("open session");
    let closed = session
        .close(1, release.created_at + Duration::seconds(1))
        .expect("closed session");
    let close = CloseApplicationSessionWrite {
        session: closed.clone(),
        expected_version: 1,
    };
    let first_close = repository
        .close_session(close.clone())
        .await
        .expect("persist close");
    assert!(!first_close.replayed);
    assert_eq!(first_close.value, closed);
    let replayed_close = repository.close_session(close).await.expect("replay close");
    assert!(replayed_close.replayed);
    assert_eq!(replayed_close.value, closed);

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Blocking,
        json!({"query": "late"}),
        release.created_at + Duration::seconds(2),
    )
    .expect("proposed invocation");
    let input =
        ApplicationMessage::input(&session, &invocation, invocation.requested_at).expect("input");
    let workflow_authority = workflow_authority(&invocation, release.created_by);
    assert!(matches!(
        repository
            .request_invocation(RequestApplicationInvocationWrite {
                invocation: invocation.clone(),
                workflow_authority,
                input_message: input,
                expected_session_version: 1,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(repository
        .find_invocation(
            invocation.organization_id,
            invocation.project_id,
            invocation.application_id,
            invocation.id,
        )
        .await
        .expect("find invocation")
        .is_none());
    assert_eq!(
        repository
            .find_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("find session"),
        Some(closed)
    );
}
