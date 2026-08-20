use crate::infrastructure::{execute, require_one_row, PostgresPersistenceError};
use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationInvocation, ApplicationMessage, ApplicationMessageKind,
    ApplicationSession, ApplicationWorkflowEffect, ConversationVariableRevision,
};
use a3s_orm::{sql_query, PostgresTransaction};
use uuid::Uuid;

pub(super) async fn insert_end_user(
    transaction: &PostgresTransaction,
    value: &ApplicationEndUser,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application end user",
        execute(
            transaction,
            sql_query::<()>("insert into application_end_users (organization_id, project_id, application_id, id, audience, linked_principal_id, created_by, created_at) values (")
                .bind(value.organization_id.as_uuid())
                .append(", ")
                .bind(value.project_id.as_uuid())
                .append(", ")
                .bind(value.application_id.as_uuid())
                .append(", ")
                .bind(value.id.as_uuid())
                .append(", ")
                .bind(value.audience.as_str())
                .append(", ")
                .bind(value.linked_principal_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(value.created_by.as_uuid())
                .append(", ")
                .bind(value.created_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn insert_session(
    transaction: &PostgresTransaction,
    value: &ApplicationSession,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application session",
        execute(
            transaction,
            sql_query::<()>("insert into application_sessions (organization_id, project_id, application_id, application_release_id, application_release_number, application_release_digest, end_user_id, id, interaction_mode, status, last_message_sequence, current_variable_revision_id, current_variable_revision_number, current_variable_digest, aggregate_version, created_at, updated_at, closed_at) values (")
                .bind(value.organization_id.as_uuid())
                .append(", ")
                .bind(value.project_id.as_uuid())
                .append(", ")
                .bind(value.application_id.as_uuid())
                .append(", ")
                .bind(value.application_release_id.as_uuid())
                .append(", ")
                .bind(value.application_release_number)
                .append(", ")
                .bind(value.application_release_digest.as_str())
                .append(", ")
                .bind(value.end_user_id.as_uuid())
                .append(", ")
                .bind(value.id.as_uuid())
                .append(", ")
                .bind(value.interaction_mode.as_str())
                .append(", ")
                .bind(value.status.as_str())
                .append(", ")
                .bind(value.last_message_sequence)
                .append(", ")
                .bind(value.current_variable_revision_id.as_uuid())
                .append(", ")
                .bind(value.current_variable_revision_number)
                .append(", ")
                .bind(value.current_variable_digest.as_str())
                .append(", ")
                .bind(value.aggregate_version)
                .append(", ")
                .bind(value.created_at)
                .append(", ")
                .bind(value.updated_at)
                .append(", ")
                .bind(value.closed_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn insert_invocation(
    transaction: &PostgresTransaction,
    value: &ApplicationInvocation,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application invocation",
        execute(
            transaction,
            sql_query::<()>("insert into application_invocations (organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, id, response_mode, input, input_digest, workflow_run_id, status, aggregate_version, requested_at, updated_at, completed_at) values (")
                .bind(value.organization_id.as_uuid())
                .append(", ")
                .bind(value.project_id.as_uuid())
                .append(", ")
                .bind(value.application_id.as_uuid())
                .append(", ")
                .bind(value.application_release_id.as_uuid())
                .append(", ")
                .bind(value.application_release_digest.as_str())
                .append(", ")
                .bind(value.session_id.as_uuid())
                .append(", ")
                .bind(value.id.as_uuid())
                .append(", ")
                .bind(value.response_mode.as_str())
                .append(", ")
                .bind(value.input.clone())
                .append(", ")
                .bind(value.input_digest.as_str())
                .append(", ")
                .bind(value.workflow_run_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(value.status.as_str())
                .append(", ")
                .bind(value.aggregate_version)
                .append(", ")
                .bind(value.requested_at)
                .append(", ")
                .bind(value.updated_at)
                .append(", ")
                .bind(value.completed_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn insert_message(
    transaction: &PostgresTransaction,
    value: &ApplicationMessage,
) -> Result<(), PostgresPersistenceError> {
    let effect = value.workflow_effect.as_ref();
    require_one_row(
        "Application message",
        execute(
            transaction,
            sql_query::<()>("insert into application_messages (organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, invocation_id, id, sequence, kind, content, content_digest, workflow_run_id, workflow_step_id, workflow_attempt, workflow_effect_ordinal, created_at) values (")
                .bind(value.organization_id.as_uuid())
                .append(", ")
                .bind(value.project_id.as_uuid())
                .append(", ")
                .bind(value.application_id.as_uuid())
                .append(", ")
                .bind(value.application_release_id.as_uuid())
                .append(", ")
                .bind(value.application_release_digest.as_str())
                .append(", ")
                .bind(value.session_id.as_uuid())
                .append(", ")
                .bind(value.invocation_id.as_uuid())
                .append(", ")
                .bind(value.id.as_uuid())
                .append(", ")
                .bind(value.sequence)
                .append(", ")
                .bind(value.kind.as_str())
                .append(", ")
                .bind(value.content.clone())
                .append(", ")
                .bind(value.content_digest.as_str())
                .append(", ")
                .bind(effect.map(|effect| effect.workflow_run_id.as_uuid()))
                .append(", ")
                .bind(effect.map(|effect| effect.step_id.as_str()))
                .append(", ")
                .bind(effect.map(|effect| effect.attempt))
                .append(", ")
                .bind(effect.map(|effect| effect.ordinal))
                .append(", ")
                .bind(value.created_at)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn insert_variable_revision(
    transaction: &PostgresTransaction,
    value: &ConversationVariableRevision,
) -> Result<(), PostgresPersistenceError> {
    let effect = value.source_effect.as_ref();
    require_one_row(
        "Application conversation variable revision",
        execute(
            transaction,
            sql_query::<()>("insert into application_conversation_variable_revisions (organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, id, revision_number, parent_revision_id, parent_digest, values_json, values_digest, workflow_run_id, workflow_step_id, workflow_attempt, workflow_effect_ordinal, created_at) values (")
                .bind(value.organization_id.as_uuid())
                .append(", ")
                .bind(value.project_id.as_uuid())
                .append(", ")
                .bind(value.application_id.as_uuid())
                .append(", ")
                .bind(value.application_release_id.as_uuid())
                .append(", ")
                .bind(value.application_release_digest.as_str())
                .append(", ")
                .bind(value.session_id.as_uuid())
                .append(", ")
                .bind(value.id.as_uuid())
                .append(", ")
                .bind(value.revision_number)
                .append(", ")
                .bind(value.parent_revision_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(value.parent_digest.as_ref().map(|digest| digest.as_str()))
                .append(", ")
                .bind(value.values.clone())
                .append(", ")
                .bind(value.values_digest.as_str())
                .append(", ")
                .bind(effect.map(|effect| effect.workflow_run_id.as_uuid()))
                .append(", ")
                .bind(effect.map(|effect| effect.step_id.as_str()))
                .append(", ")
                .bind(effect.map(|effect| effect.attempt))
                .append(", ")
                .bind(effect.map(|effect| effect.ordinal))
                .append(", ")
                .bind(value.created_at)
                .append(")"),
        )
        .await?,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_effect_claim(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    effect: &ApplicationWorkflowEffect,
    semantic_kind: &str,
    semantic_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application Workflow effect claim",
        execute(
            transaction,
            sql_query::<()>("insert into application_workflow_effect_claims (organization_id, project_id, application_id, session_id, workflow_run_id, workflow_step_id, workflow_attempt, workflow_effect_ordinal, semantic_kind, semantic_id) values (")
                .bind(organization_id)
                .append(", ")
                .bind(project_id)
                .append(", ")
                .bind(application_id)
                .append(", ")
                .bind(session_id)
                .append(", ")
                .bind(effect.workflow_run_id.as_uuid())
                .append(", ")
                .bind(effect.step_id.as_str())
                .append(", ")
                .bind(effect.attempt)
                .append(", ")
                .bind(effect.ordinal)
                .append(", ")
                .bind(semantic_kind)
                .append(", ")
                .bind(semantic_id)
                .append(")"),
        )
        .await?,
    )
}

pub(super) async fn update_session(
    transaction: &PostgresTransaction,
    value: &ApplicationSession,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application session head",
        execute(
            transaction,
            sql_query::<()>("update application_sessions set status = ")
                .bind(value.status.as_str())
                .append(", last_message_sequence = ")
                .bind(value.last_message_sequence)
                .append(", current_variable_revision_id = ")
                .bind(value.current_variable_revision_id.as_uuid())
                .append(", current_variable_revision_number = ")
                .bind(value.current_variable_revision_number)
                .append(", current_variable_digest = ")
                .bind(value.current_variable_digest.as_str())
                .append(", aggregate_version = ")
                .bind(value.aggregate_version)
                .append(", updated_at = ")
                .bind(value.updated_at)
                .append(", closed_at = ")
                .bind(value.closed_at)
                .append(" where organization_id = ")
                .bind(value.organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(value.project_id.as_uuid())
                .append(" and application_id = ")
                .bind(value.application_id.as_uuid())
                .append(" and id = ")
                .bind(value.id.as_uuid())
                .append(" and aggregate_version = ")
                .bind(expected_version),
        )
        .await?,
    )
}

pub(super) async fn update_invocation(
    transaction: &PostgresTransaction,
    value: &ApplicationInvocation,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application invocation",
        execute(
            transaction,
            sql_query::<()>("update application_invocations set workflow_run_id = ")
                .bind(value.workflow_run_id.map(|id| id.as_uuid()))
                .append(", status = ")
                .bind(value.status.as_str())
                .append(", aggregate_version = ")
                .bind(value.aggregate_version)
                .append(", updated_at = ")
                .bind(value.updated_at)
                .append(", completed_at = ")
                .bind(value.completed_at)
                .append(" where organization_id = ")
                .bind(value.organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(value.project_id.as_uuid())
                .append(" and application_id = ")
                .bind(value.application_id.as_uuid())
                .append(" and id = ")
                .bind(value.id.as_uuid())
                .append(" and aggregate_version = ")
                .bind(expected_version),
        )
        .await?,
    )
}

pub(super) const fn message_effect_kind(kind: ApplicationMessageKind) -> &'static str {
    match kind {
        ApplicationMessageKind::Input => "input",
        ApplicationMessageKind::Answer => "message_answer",
        ApplicationMessageKind::FinalOutput => "message_final_output",
    }
}
