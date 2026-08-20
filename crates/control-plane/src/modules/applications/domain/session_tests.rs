use super::*;
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
    ApplicationSessionId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

pub(super) fn make_release(
    audience: ApplicationAudience,
    interaction_mode: ApplicationInteractionMode,
) -> ApplicationRelease {
    let experience = match interaction_mode {
        ApplicationInteractionMode::Conversation => ApplicationExperience::Chatflow,
        ApplicationInteractionMode::Invocation => ApplicationExperience::Workflow,
    };
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience,
        audience,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode,
            response_modes: vec![
                ApplicationResponseMode::Blocking,
                ApplicationResponseMode::Streaming,
            ],
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
    ApplicationRelease::initial(
        OrganizationId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000001").expect("UUID"),
        ),
        ProjectId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000002").expect("UUID"),
        ),
        ApplicationId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000003").expect("UUID"),
        ),
        ApplicationReleaseId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000004").expect("UUID"),
        ),
        contract,
        PrincipalId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000005").expect("UUID"),
        ),
        Utc.with_ymd_and_hms(2026, 8, 21, 8, 0, 0)
            .single()
            .expect("timestamp"),
    )
    .expect("release")
}

fn end_user(release: &ApplicationRelease) -> ApplicationEndUser {
    let linked_principal_id = match release.contract.spec().audience {
        ApplicationAudience::ProjectMembers => Some(PrincipalId::new()),
        ApplicationAudience::AuthenticatedEndUsers | ApplicationAudience::Anonymous => None,
    };
    ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        release,
        linked_principal_id,
        release.created_by,
        release.created_at,
    )
    .expect("end user")
}

pub(super) fn session_state(
    release: &ApplicationRelease,
) -> (
    ApplicationEndUser,
    ApplicationSession,
    ConversationVariableRevision,
) {
    let end_user = end_user(release);
    let session_id = ApplicationSessionId::from_uuid(
        Uuid::parse_str("018f0000-0000-7000-8000-000000000010").expect("UUID"),
    );
    let variables = ConversationVariableRevision::initial(
        session_id,
        release,
        json!({"locale": "en-US"}),
        release.created_at,
    )
    .expect("variables");
    let session = ApplicationSession::create(
        session_id,
        release,
        &end_user,
        &variables,
        release.created_at,
    )
    .expect("session");
    (end_user, session, variables)
}

#[test]
fn end_users_cannot_turn_caller_controlled_identity_into_workspace_authority() {
    let project = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    assert!(ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &project,
        None,
        project.created_by,
        project.created_at,
    )
    .is_err());

    let anonymous = make_release(
        ApplicationAudience::Anonymous,
        ApplicationInteractionMode::Conversation,
    );
    assert!(ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &anonymous,
        Some(PrincipalId::new()),
        anonymous.created_by,
        anonymous.created_at,
    )
    .is_err());

    let authenticated = make_release(
        ApplicationAudience::AuthenticatedEndUsers,
        ApplicationInteractionMode::Conversation,
    );
    let linked = ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &authenticated,
        Some(PrincipalId::new()),
        authenticated.created_by,
        authenticated.created_at,
    )
    .expect("explicit Principal link");
    assert!(linked.linked_principal_id.is_some());
}

#[test]
fn session_and_invocation_pin_one_exact_release_and_admitted_response_mode() {
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (_, session, _) = session_state(&release);
    assert_eq!(session.application_release_id, release.id);
    assert_eq!(
        session.application_release_digest,
        *release.contract.digest()
    );
    assert_eq!(session.current_variable_revision_number, 1);

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        release.created_at,
    )
    .expect("invocation");
    assert_eq!(invocation.status, ApplicationInvocationStatus::Requested);
    assert_eq!(
        invocation.input_digest,
        digest_json(&json!({"query": "hello"})).expect("input digest")
    );

    let asynchronous = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Asynchronous,
        json!({"query": "hello"}),
        release.created_at,
    );
    assert!(asynchronous.is_err());

    let foreign = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    assert!(ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &foreign,
        ApplicationResponseMode::Blocking,
        json!({"query": "hello"}),
        release.created_at,
    )
    .is_err());
}

#[test]
fn ordered_messages_use_deterministic_workflow_effect_identities() {
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (_, session, _) = session_state(&release);
    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000020").expect("UUID"),
        ),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        release.created_at,
    )
    .expect("invocation");
    let input = ApplicationMessage::input(&session, &invocation, release.created_at)
        .expect("input message");
    let session = session.append_message(1, &input).expect("append input");
    assert_eq!(input.sequence, 1);
    assert_eq!(input.kind, ApplicationMessageKind::Input);

    let run_id = WorkflowRunId::from_uuid(
        Uuid::parse_str("018f0000-0000-7000-8000-000000000030").expect("UUID"),
    );
    let invocation = invocation
        .bind_workflow_run(1, run_id, release.created_at + Duration::seconds(1))
        .expect("bind run");
    let effect = ApplicationWorkflowEffect::new(run_id, "answer", 1, 0).expect("effect");
    let first = ApplicationMessage::workflow_frame(
        &session,
        &invocation,
        ApplicationMessageKind::Answer,
        effect.clone(),
        json!({"text": "Hello"}),
        release.created_at + Duration::seconds(2),
    )
    .expect("Answer frame");
    let replay = ApplicationMessage::workflow_frame(
        &session,
        &invocation,
        ApplicationMessageKind::Answer,
        effect,
        json!({"text": "Hello"}),
        release.created_at + Duration::seconds(2),
    )
    .expect("replayed Answer frame");
    assert_eq!(first.id, replay.id);
    assert_eq!(first, replay);
    assert_eq!(first.sequence, 2);

    let session = session.append_message(2, &first).expect("append Answer");
    assert_eq!(session.last_message_sequence, 2);
    assert!(session.append_message(2, &first).is_err());
}

#[test]
fn conversation_variable_revisions_are_optimistic_and_effect_idempotent() {
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (_, session, initial) = session_state(&release);
    let run_id = WorkflowRunId::new();
    let effect = ApplicationWorkflowEffect::new(run_id, "assign", 2, 0).expect("effect");
    let successor = ConversationVariableRevision::successor(
        &initial,
        effect.clone(),
        json!({"locale": "en-US", "topic": "billing"}),
        release.created_at + Duration::seconds(1),
    )
    .expect("successor");
    let replay = ConversationVariableRevision::successor(
        &initial,
        effect,
        json!({"topic": "billing", "locale": "en-US"}),
        release.created_at + Duration::seconds(1),
    )
    .expect("canonical replay");
    assert_eq!(successor, replay);
    assert_eq!(successor.revision_number, 2);

    let advanced = session
        .advance_variables(1, &successor)
        .expect("advance variables");
    assert_eq!(advanced.current_variable_revision_number, 2);
    assert_eq!(advanced.current_variable_digest, successor.values_digest);
    assert!(session.advance_variables(2, &successor).is_err());
    assert!(ConversationVariableRevision::successor(
        &initial,
        ApplicationWorkflowEffect::new(run_id, "noop", 1, 0).expect("effect"),
        initial.values.clone(),
        release.created_at + Duration::seconds(1),
    )
    .is_err());
}

#[test]
fn closed_sessions_reject_messages_variables_and_regressing_time() {
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (_, session, initial) = session_state(&release);
    let closed = session
        .close(1, release.created_at + Duration::seconds(2))
        .expect("close");
    assert_eq!(closed.status, ApplicationSessionStatus::Closed);
    assert!(closed.close(2, release.created_at).is_err());

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Blocking,
        json!({"query": "hello"}),
        release.created_at,
    )
    .expect("invocation");
    let message =
        ApplicationMessage::input(&session, &invocation, release.created_at).expect("message");
    assert!(closed.append_message(2, &message).is_err());

    let revision = ConversationVariableRevision::successor(
        &initial,
        ApplicationWorkflowEffect::new(WorkflowRunId::new(), "assign", 1, 0).expect("effect"),
        json!({"locale": "fr-FR"}),
        release.created_at + Duration::seconds(3),
    )
    .expect("revision");
    assert!(closed.advance_variables(2, &revision).is_err());
}

#[test]
fn invocation_cancellation_and_terminal_observation_preserve_flow_authority() {
    let release = make_release(
        ApplicationAudience::ProjectMembers,
        ApplicationInteractionMode::Conversation,
    );
    let (_, session, _) = session_state(&release);
    let requested = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Blocking,
        json!({"query": "cancel"}),
        release.created_at,
    )
    .expect("requested invocation");
    let cancelling = requested
        .request_cancellation(1, release.created_at + Duration::seconds(1))
        .expect("cancelling invocation");
    assert_eq!(cancelling.aggregate_version, 2);
    assert!(cancelling.workflow_run_id.is_none());
    let cancelled = cancelling
        .observe_terminal(
            2,
            ApplicationInvocationStatus::Cancelled,
            release.created_at + Duration::seconds(2),
        )
        .expect("cancelled invocation");
    assert_eq!(cancelled.aggregate_version, 3);
    assert!(cancelled.status.is_terminal());
    assert!(cancelled
        .request_cancellation(3, cancelled.updated_at)
        .is_err());

    let run_id = WorkflowRunId::new();
    let running = requested
        .bind_workflow_run(1, run_id, release.created_at + Duration::seconds(1))
        .expect("running invocation");
    let failed = running
        .observe_terminal(
            2,
            ApplicationInvocationStatus::Failed,
            release.created_at + Duration::seconds(2),
        )
        .expect("failed invocation");
    assert_eq!(failed.aggregate_version, 3);
    assert!(ApplicationMessage::workflow_frame(
        &session,
        &failed,
        ApplicationMessageKind::FinalOutput,
        ApplicationWorkflowEffect::new(run_id, "output", 1, 0).expect("effect"),
        json!({"result": "invalid"}),
        failed.updated_at,
    )
    .is_err());

    let mut corrupt = failed;
    corrupt.aggregate_version = 99;
    assert!(corrupt.restore().is_err());
}

#[test]
fn application_session_contract_does_not_duplicate_execution_authority() {
    let implementation = [
        include_str!("application_end_user.rs"),
        include_str!("application_effect.rs"),
        include_str!("application_invocation.rs"),
        include_str!("application_message.rs"),
        include_str!("application_session.rs"),
        include_str!("conversation_variables.rs"),
        include_str!("session_repository.rs"),
        include_str!("../infrastructure/session_in_memory.rs"),
        include_str!("../infrastructure/session_in_memory_state.rs"),
    ]
    .join("\n");
    for forbidden in [
        "a3s_flow",
        "FlowRuntime",
        "reqwest",
        "tokio::spawn",
        "modules::agents",
        "modules::secrets",
        "modules::workloads",
    ] {
        assert!(
            !implementation.contains(forbidden),
            "Application session core duplicates authority through {forbidden}"
        );
    }

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplicationEndUser>();
    assert_send_sync::<ApplicationSession>();
    assert_send_sync::<ApplicationInvocation>();
    assert_send_sync::<ApplicationMessage>();
    assert_send_sync::<ConversationVariableRevision>();
    assert_send_sync::<
        crate::modules::applications::infrastructure::InMemoryApplicationSessionRepository,
    >();
}
