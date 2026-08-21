use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationInvocation, ApplicationInvocationWorkflowAuthority,
    ApplicationMessage, ApplicationSession, ApplicationWorkflowEffect,
    ConversationVariableRevision,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, ProjectId,
    WorkflowRunId,
};
use a3s_orm::{sql_query, PostgresTransaction};
use uuid::Uuid;

use super::session_postgres_records::{
    decode_end_user, decode_invocation, decode_invocation_workflow_authority, decode_message,
    decode_session, decode_variable, end_user_select, invocation_select,
    invocation_workflow_authority_select, message_select, session_select, variable_select,
};

pub(super) async fn lock_session(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
) -> Result<Option<ApplicationSession>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        session_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and application_id = ")
            .bind(application_id.as_uuid())
            .append(" and id = ")
            .bind(session_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_session)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn lock_invocation(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    invocation_id: ApplicationInvocationId,
) -> Result<Option<ApplicationInvocation>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        invocation_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and application_id = ")
            .bind(application_id.as_uuid())
            .append(" and id = ")
            .bind(invocation_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_invocation)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_end_user(
    transaction: &PostgresTransaction,
    value: &ApplicationEndUser,
) -> Result<Option<ApplicationEndUser>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        end_user_select()
            .append(" where organization_id = ")
            .bind(value.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(value.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(value.application_id.as_uuid())
            .append(" and id = ")
            .bind(value.id.as_uuid()),
    )
    .await?
    .map(decode_end_user)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_message(
    transaction: &PostgresTransaction,
    value: &ApplicationMessage,
) -> Result<Option<ApplicationMessage>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        message_select()
            .append(" where organization_id = ")
            .bind(value.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(value.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(value.application_id.as_uuid())
            .append(" and id = ")
            .bind(value.id.as_uuid()),
    )
    .await?
    .map(decode_message)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_invocation_workflow_authority(
    transaction: &PostgresTransaction,
    value: &ApplicationInvocationWorkflowAuthority,
) -> Result<Option<ApplicationInvocationWorkflowAuthority>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        invocation_workflow_authority_select()
            .append(" where organization_id = ")
            .bind(value.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(value.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(value.application_id.as_uuid())
            .append(" and invocation_id = ")
            .bind(value.invocation_id.as_uuid()),
    )
    .await?
    .map(decode_invocation_workflow_authority)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_variable(
    transaction: &PostgresTransaction,
    value: &ConversationVariableRevision,
) -> Result<Option<ConversationVariableRevision>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        variable_select()
            .append(" where organization_id = ")
            .bind(value.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(value.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(value.application_id.as_uuid())
            .append(" and session_id = ")
            .bind(value.session_id.as_uuid())
            .append(" and id = ")
            .bind(value.id.as_uuid()),
    )
    .await?
    .map(decode_variable)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_variable_head(
    transaction: &PostgresTransaction,
    session: &ApplicationSession,
) -> Result<Option<ConversationVariableRevision>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        variable_select()
            .append(" where organization_id = ")
            .bind(session.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(session.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(session.application_id.as_uuid())
            .append(" and session_id = ")
            .bind(session.id.as_uuid())
            .append(" and id = ")
            .bind(session.current_variable_revision_id.as_uuid()),
    )
    .await?
    .map(decode_variable)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_invocation_for_run(
    transaction: &PostgresTransaction,
    revision: &ConversationVariableRevision,
    workflow_run_id: WorkflowRunId,
) -> Result<Option<ApplicationInvocation>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        invocation_select()
            .append(" where organization_id = ")
            .bind(revision.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(revision.project_id.as_uuid())
            .append(" and application_id = ")
            .bind(revision.application_id.as_uuid())
            .append(" and session_id = ")
            .bind(revision.session_id.as_uuid())
            .append(" and workflow_run_id = ")
            .bind(workflow_run_id.as_uuid()),
    )
    .await?
    .map(decode_invocation)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_effect_claim(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    effect: &ApplicationWorkflowEffect,
) -> Result<Option<(String, Uuid)>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        sql_query::<(String, Uuid)>(
            "select semantic_kind, semantic_id from application_workflow_effect_claims where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and application_id = ")
        .bind(application_id.as_uuid())
        .append(" and session_id = ")
        .bind(session_id.as_uuid())
        .append(" and workflow_run_id = ")
        .bind(effect.workflow_run_id.as_uuid())
        .append(" and workflow_step_id = ")
        .bind(effect.step_id.as_str())
        .append(" and workflow_attempt = ")
        .bind(effect.attempt)
        .append(" and workflow_effect_ordinal = ")
        .bind(effect.ordinal),
    )
    .await
}

pub(super) async fn has_final_output(
    transaction: &PostgresTransaction,
    message: &ApplicationMessage,
) -> Result<bool, PostgresPersistenceError> {
    Ok(fetch_optional::<bool, _>(
        transaction,
        sql_query::<bool>(
            "select exists (select 1 from application_messages where organization_id = ",
        )
        .bind(message.organization_id.as_uuid())
        .append(" and application_id = ")
        .bind(message.application_id.as_uuid())
        .append(" and invocation_id = ")
        .bind(message.invocation_id.as_uuid())
        .append(" and kind = 'final_output')"),
    )
    .await?
    .unwrap_or(false))
}
