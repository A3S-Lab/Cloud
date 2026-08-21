use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationInvocation, ApplicationInvocationWorkflowAuthority,
    ApplicationMessage, ApplicationSession, ConversationVariableRevision,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationSessionId,
    ConversationVariableRevisionId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::{Database, PostgresDialect, PostgresExecutor};

use super::session_postgres_records::{
    decode_end_user, decode_invocation, decode_invocation_workflow_authority, decode_message,
    decode_session, decode_variable, end_user_select, invocation_select,
    invocation_workflow_authority_select, message_select, session_select, variable_select,
};

pub(super) async fn find_end_user(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    end_user_id: ApplicationEndUserId,
) -> Result<Option<ApplicationEndUser>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            end_user_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and id = ")
                .bind(end_user_id.as_uuid()),
        )
        .await
        .map_err(database_error)?
        .map(decode_end_user)
        .transpose()
}

pub(super) async fn find_session(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
) -> Result<Option<ApplicationSession>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            session_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and id = ")
                .bind(session_id.as_uuid()),
        )
        .await
        .map_err(database_error)?
        .map(decode_session)
        .transpose()
}

pub(super) async fn find_invocation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    invocation_id: ApplicationInvocationId,
) -> Result<Option<ApplicationInvocation>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            invocation_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and id = ")
                .bind(invocation_id.as_uuid()),
        )
        .await
        .map_err(database_error)?
        .map(decode_invocation)
        .transpose()
}

pub(super) async fn find_invocation_workflow_authority(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    invocation_id: ApplicationInvocationId,
) -> Result<Option<ApplicationInvocationWorkflowAuthority>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            invocation_workflow_authority_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and invocation_id = ")
                .bind(invocation_id.as_uuid()),
        )
        .await
        .map_err(database_error)?
        .map(decode_invocation_workflow_authority)
        .transpose()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn list_messages(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    after_sequence: u64,
    limit: usize,
) -> Result<Vec<ApplicationMessage>, RepositoryError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            message_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and session_id = ")
                .bind(session_id.as_uuid())
                .append(" and sequence > ")
                .bind(after_sequence)
                .append(" order by sequence asc, id asc limit ")
                .bind(limit),
        )
        .await
        .map_err(database_error)?
        .rows
        .into_iter()
        .map(decode_message)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn find_variable_revision(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    revision_id: ConversationVariableRevisionId,
) -> Result<Option<ConversationVariableRevision>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            variable_select()
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and session_id = ")
                .bind(session_id.as_uuid())
                .append(" and id = ")
                .bind(revision_id.as_uuid()),
        )
        .await
        .map_err(database_error)?
        .map(decode_variable)
        .transpose()
}

fn database_error(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
