use super::application_session_fixtures_support::{persist_application_release, seed_workflow_run};
use super::applications_support::{digest, seed_scope, seed_workflow_revision};
use super::*;
use a3s_cloud_control_plane::modules::applications::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, ApplicationEndUser, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    ApplicationMessageKind, ApplicationResponseMode, ApplicationSession, ApplicationWorkflowEffect,
    CloseApplicationSessionWrite, ConversationVariableRevision, IApplicationSessionRepository,
    OpenApplicationSessionWrite, PostgresApplicationSessionRepository,
    RequestApplicationInvocationWrite,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationInvocationId, ApplicationSessionId, OntologyId,
    OntologyRevisionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_orm::{Database, PostgresDialect};
use chrono::{Duration, Utc};
use serde_json::json;

pub(super) async fn exercise_application_session_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, String)>(
                    "select count(*), max(name) from a3s_orm_migrations where version = ",
                )
                .bind("125"),
            )
            .await?,
        (1, "Application sessions and semantic effects".into())
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, String)>(
                    "select count(*), max(name) from a3s_orm_migrations where version = ",
                )
                .bind("126"),
            )
            .await?,
        (1, "Application invocation Workflow authority".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let workflow_definition_id = WorkflowDefinitionId::new();
    let workflow_revision_id = WorkflowRevisionId::new();
    let workflow_contract_digest = digest('a');
    let workflow_payload_set_digest = digest('b');
    let created_at = Utc::now();
    seed_scope(&database, organization_id, project_id, actor, created_at).await?;
    seed_workflow_revision(
        &executor,
        organization_id,
        project_id,
        actor,
        workflow_definition_id,
        workflow_revision_id,
        &workflow_contract_digest,
        &workflow_payload_set_digest,
        created_at,
    )
    .await?;
    let (application, release) = persist_application_release(
        &executor,
        organization_id,
        project_id,
        actor,
        workflow_definition_id,
        workflow_revision_id,
        workflow_contract_digest,
        workflow_payload_set_digest,
        created_at + Duration::seconds(1),
    )
    .await?;

    let repository = PostgresApplicationSessionRepository::new(executor.clone());
    let opened_at = created_at + Duration::seconds(2);
    let end_user = ApplicationEndUser::create(
        ApplicationEndUserId::new(),
        &release,
        Some(actor),
        actor,
        opened_at,
    )?;
    let session_id = ApplicationSessionId::new();
    let initial_variables = ConversationVariableRevision::initial(
        session_id,
        &release,
        json!({"locale": "en-US"}),
        opened_at,
    )?;
    let session = ApplicationSession::create(
        session_id,
        &release,
        &end_user,
        &initial_variables,
        opened_at,
    )?;
    let open = OpenApplicationSessionWrite {
        release: release.clone(),
        end_user: end_user.clone(),
        session: session.clone(),
        initial_variables: initial_variables.clone(),
    };
    assert!(!repository.open_session(open.clone()).await?.replayed);
    assert!(
        PostgresApplicationSessionRepository::new(executor.clone())
            .open_session(open.clone())
            .await?
            .replayed
    );

    let invocation = ApplicationInvocation::request(
        ApplicationInvocationId::new(),
        &session,
        &release,
        ApplicationResponseMode::Streaming,
        json!({"query": "hello"}),
        created_at + Duration::seconds(3),
    )?;
    let seeded_workflow = seed_workflow_run(
        &executor,
        organization_id,
        project_id,
        actor,
        workflow_definition_id,
        workflow_revision_id,
        created_at + Duration::seconds(3),
    )
    .await?;
    let workflow_authority = ApplicationInvocationWorkflowAuthority::new(
        &invocation,
        seeded_workflow.ontology_id,
        seeded_workflow.ontology_revision_id,
        seeded_workflow.ontology_digest,
        None,
        actor,
        3_600,
    )?;
    let input_message = ApplicationMessage::input(&session, &invocation, invocation.requested_at)?;
    let orphan_authority = ApplicationInvocationWorkflowAuthority::new(
        &invocation,
        OntologyId::new(),
        OntologyRevisionId::new(),
        digest('8'),
        None,
        actor,
        3_600,
    )?;
    assert!(
        repository
            .request_invocation(RequestApplicationInvocationWrite {
                invocation: invocation.clone(),
                workflow_authority: orphan_authority,
                input_message: input_message.clone(),
                expected_session_version: 1,
            })
            .await
            .is_err(),
        "unknown Ontology authority must abort the complete invocation transaction"
    );
    assert!(repository
        .find_invocation(organization_id, project_id, application.id, invocation.id)
        .await?
        .is_none());
    assert!(repository
        .find_invocation_workflow_authority(
            organization_id,
            project_id,
            application.id,
            invocation.id,
        )
        .await?
        .is_none());
    assert!(repository
        .list_messages(
            organization_id,
            project_id,
            application.id,
            session.id,
            0,
            10,
        )
        .await?
        .is_empty());
    assert_eq!(
        repository
            .find_session(organization_id, project_id, application.id, session.id)
            .await?,
        Some(session.clone())
    );
    let request = RequestApplicationInvocationWrite {
        invocation: invocation.clone(),
        workflow_authority: workflow_authority.clone(),
        input_message: input_message.clone(),
        expected_session_version: 1,
    };
    assert!(
        !repository
            .request_invocation(request.clone())
            .await?
            .replayed
    );

    let workflow_run_id = seeded_workflow.run_id;
    let running =
        invocation.bind_workflow_run(1, workflow_run_id, created_at + Duration::seconds(4))?;
    let bind = AdvanceApplicationInvocationWrite {
        invocation: running.clone(),
        expected_version: 1,
    };
    assert!(!repository.advance_invocation(bind.clone()).await?.replayed);
    assert!(
        PostgresApplicationSessionRepository::new(executor.clone())
            .advance_invocation(bind)
            .await?
            .replayed
    );

    let after_input = repository
        .find_session(organization_id, project_id, application.id, session.id)
        .await?
        .expect("persisted Application session");
    let answer_effect = ApplicationWorkflowEffect::new(workflow_run_id, "answer", 1, 0)?;
    let answer = ApplicationMessage::workflow_frame(
        &after_input,
        &running,
        ApplicationMessageKind::Answer,
        answer_effect.clone(),
        json!({"text": "Hello"}),
        created_at + Duration::seconds(5),
    )?;
    let append_answer = AppendApplicationMessageWrite {
        message: answer.clone(),
        expected_session_version: after_input.aggregate_version,
    };
    assert!(
        !repository
            .append_message(append_answer.clone())
            .await?
            .replayed
    );
    assert!(
        PostgresApplicationSessionRepository::new(executor.clone())
            .append_message(append_answer)
            .await?
            .replayed
    );

    let after_answer = repository
        .find_session(organization_id, project_id, application.id, session.id)
        .await?
        .expect("session after Answer");
    let cross_kind = ConversationVariableRevision::successor(
        &initial_variables,
        answer_effect,
        json!({"locale": "fr-FR"}),
        created_at + Duration::seconds(6),
    )?;
    assert!(matches!(
        repository
            .advance_variables(AdvanceConversationVariablesWrite {
                revision: cross_kind,
                expected_session_version: after_answer.aggregate_version,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let variable_effect = ApplicationWorkflowEffect::new(workflow_run_id, "assign", 1, 0)?;
    let variables = ConversationVariableRevision::successor(
        &initial_variables,
        variable_effect,
        json!({"locale": "en-US", "topic": "billing"}),
        created_at + Duration::seconds(6),
    )?;
    let advance_variables = AdvanceConversationVariablesWrite {
        revision: variables.clone(),
        expected_session_version: after_answer.aggregate_version,
    };
    assert!(
        !repository
            .advance_variables(advance_variables.clone())
            .await?
            .replayed
    );
    assert!(
        PostgresApplicationSessionRepository::new(executor.clone())
            .advance_variables(advance_variables)
            .await?
            .replayed
    );

    let before_final = repository
        .find_session(organization_id, project_id, application.id, session.id)
        .await?
        .expect("session before final output");
    let final_output = ApplicationMessage::workflow_frame(
        &before_final,
        &running,
        ApplicationMessageKind::FinalOutput,
        ApplicationWorkflowEffect::new(workflow_run_id, "output", 1, 0)?,
        json!({"result": "Hello"}),
        created_at + Duration::seconds(7),
    )?;
    let final_output_id = final_output.id;
    repository
        .append_message(AppendApplicationMessageWrite {
            message: final_output.clone(),
            expected_session_version: before_final.aggregate_version,
        })
        .await?;
    let after_final = repository
        .find_session(organization_id, project_id, application.id, session.id)
        .await?
        .expect("session after final output");
    let late_answer = ApplicationMessage::workflow_frame(
        &after_final,
        &running,
        ApplicationMessageKind::Answer,
        ApplicationWorkflowEffect::new(workflow_run_id, "answer", 1, 1)?,
        json!({"text": "late"}),
        created_at + Duration::seconds(8),
    )?;
    assert!(matches!(
        repository
            .append_message(AppendApplicationMessageWrite {
                message: late_answer,
                expected_session_version: after_final.aggregate_version,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let succeeded = running.observe_terminal(
        2,
        ApplicationInvocationStatus::Succeeded,
        created_at + Duration::seconds(8),
    )?;
    repository
        .advance_invocation(AdvanceApplicationInvocationWrite {
            invocation: succeeded.clone(),
            expected_version: 2,
        })
        .await?;
    let closed = after_final.close(
        after_final.aggregate_version,
        created_at + Duration::seconds(9),
    )?;
    repository
        .close_session(CloseApplicationSessionWrite {
            session: closed.clone(),
            expected_version: after_final.aggregate_version,
        })
        .await?;

    let restarted = PostgresApplicationSessionRepository::new(executor.clone());
    assert_eq!(
        restarted
            .find_end_user(organization_id, project_id, application.id, end_user.id)
            .await?,
        Some(end_user)
    );
    assert_eq!(
        restarted
            .find_session(organization_id, project_id, application.id, session.id)
            .await?,
        Some(closed.clone())
    );
    assert_eq!(
        restarted
            .find_invocation(organization_id, project_id, application.id, invocation.id)
            .await?,
        Some(succeeded.clone())
    );
    assert_eq!(
        restarted
            .find_invocation_workflow_authority(
                organization_id,
                project_id,
                application.id,
                invocation.id,
            )
            .await?,
        Some(workflow_authority)
    );
    assert_eq!(
        restarted
            .list_messages(
                organization_id,
                project_id,
                application.id,
                session.id,
                0,
                50
            )
            .await?,
        vec![input_message, answer.clone(), final_output.clone()]
    );
    assert_eq!(
        restarted
            .find_variable_revision(
                organization_id,
                project_id,
                application.id,
                session.id,
                variables.id,
            )
            .await?,
        Some(variables.clone())
    );
    assert!(restarted.open_session(open).await?.replayed);
    assert!(restarted.request_invocation(request).await?.replayed);
    assert!(
        restarted
            .append_message(AppendApplicationMessageWrite {
                message: answer,
                expected_session_version: 2,
            })
            .await?
            .replayed
    );
    assert!(
        restarted
            .append_message(AppendApplicationMessageWrite {
                message: final_output,
                expected_session_version: 4,
            })
            .await?
            .replayed
    );

    let counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from application_end_users where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_sessions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_invocations where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_invocation_workflow_authorities where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_messages where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_conversation_variable_revisions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_workflow_effect_claims where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(")"),
        )
        .await?;
    assert_eq!(counts, (1, 1, 1, 1, 3, 2, 3));
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update application_invocation_workflow_authorities set timeout_seconds = timeout_seconds + 1 where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application.id.as_uuid())
                    .append(" and invocation_id = ")
                    .bind(invocation.id.as_uuid()),
            )
            .await,
        "mutating immutable Application invocation Workflow authority",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update application_messages set content = '{}'::jsonb where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application.id.as_uuid())
                    .append(" and id = ")
                    .bind(final_output_id.as_uuid()),
            )
            .await,
        "mutating an immutable Application message",
    );
    Ok(())
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
