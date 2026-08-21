use super::resource_access::project;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationEndUser, ApplicationSession, IApplicationSessionRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationSessionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AuthorizedApplicationSession {
    pub end_user: ApplicationEndUser,
    pub session: ApplicationSession,
}

/// Authorize the project before reading replay state, then narrow delivery
/// access to the exact Principal-linked project-member end user.
pub(super) async fn project_member_session(
    sessions: &dyn IApplicationSessionRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    actor_principal_id: PrincipalId,
    resource_access: &ResourceAccessEvaluator,
) -> ApplicationResult<AuthorizedApplicationSession> {
    project(project_id, resource_access)?;
    if organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || application_id.as_uuid().is_nil()
        || session_id.as_uuid().is_nil()
        || actor_principal_id.as_uuid().is_nil()
    {
        return Err(ApplicationError::Invalid(
            "Application delivery scope is invalid".into(),
        ));
    }
    let session = match sessions
        .find_session(organization_id, project_id, application_id, session_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(session_not_found()),
        Err(error) => return Err(error.into()),
    };
    session.validate().map_err(ApplicationError::Internal)?;
    if session.organization_id != organization_id
        || session.project_id != project_id
        || session.application_id != application_id
        || session.id != session_id
    {
        return Err(session_not_found());
    }
    let end_user = match sessions
        .find_end_user(
            organization_id,
            project_id,
            application_id,
            session.end_user_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => {
            return Err(ApplicationError::Internal(
                "Application session end user is missing".into(),
            ));
        }
        Err(error) => return Err(error.into()),
    };
    end_user.validate().map_err(ApplicationError::Internal)?;
    if end_user.organization_id != session.organization_id
        || end_user.project_id != session.project_id
        || end_user.application_id != session.application_id
        || end_user.id != session.end_user_id
        || end_user.created_at > session.created_at
    {
        return Err(ApplicationError::Internal(
            "Application session end user drifted from its owner".into(),
        ));
    }
    if end_user.audience != ApplicationAudience::ProjectMembers
        || end_user.linked_principal_id != Some(actor_principal_id)
        || end_user.id
            != ApplicationEndUser::project_member_id(application_id, actor_principal_id)
                .map_err(ApplicationError::Invalid)?
    {
        return Err(session_not_found());
    }
    Ok(AuthorizedApplicationSession { end_user, session })
}

pub(super) fn session_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application session not found".into())
}

pub(super) fn invocation_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application invocation not found".into())
}
