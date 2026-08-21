use super::delivery_access::{invocation_not_found, project_member_session};
use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationInvocation, ApplicationMessage, ApplicationSession,
    ConversationVariableRevision, IApplicationSessionRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, PrincipalId,
    ProjectId, RepositoryError,
};
use a3s_boot::{Query, QueryHandler};
use serde::Serialize;
use std::sync::Arc;

pub const DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT: usize = 100;
pub const MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT: usize = 500;

#[derive(Debug, Clone)]
pub struct GetApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetApplicationSession {
    type Output = ApplicationResult<GetApplicationSessionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetApplicationSessionResult {
    pub end_user: ApplicationEndUser,
    pub session: ApplicationSession,
    pub current_variables: ConversationVariableRevision,
}

pub struct GetApplicationSessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl GetApplicationSessionHandler {
    pub fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self { sessions }
    }
}

impl QueryHandler<GetApplicationSession> for GetApplicationSessionHandler {
    fn execute(
        &self,
        query: GetApplicationSession,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GetApplicationSessionResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
                query.actor_principal_id,
                &query.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let current_variables =
                match load_current_variables(sessions.as_ref(), &access.session).await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            Ok(Ok(GetApplicationSessionResult {
                end_user: access.end_user,
                session: access.session,
                current_variables,
            }))
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetApplicationInvocation {
    type Output = ApplicationResult<ApplicationInvocation>;
}

pub struct GetApplicationInvocationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl GetApplicationInvocationHandler {
    pub fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self { sessions }
    }
}

impl QueryHandler<GetApplicationInvocation> for GetApplicationInvocationHandler {
    fn execute(
        &self,
        query: GetApplicationInvocation,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationInvocation>>>
    {
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
                query.actor_principal_id,
                &query.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if query.invocation_id.as_uuid().is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "Application invocation identity is invalid".into(),
                )));
            }
            match sessions
                .find_invocation(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.invocation_id,
                )
                .await
            {
                Ok(Some(value)) if value.session_id == query.session_id => {
                    if let Err(error) = value.validate() {
                        return Ok(Err(ApplicationError::Internal(error)));
                    }
                    if value.organization_id != access.session.organization_id
                        || value.project_id != access.session.project_id
                        || value.application_id != access.session.application_id
                        || value.application_release_id != access.session.application_release_id
                        || value.application_release_digest
                            != access.session.application_release_digest
                    {
                        return Ok(Err(ApplicationError::Internal(
                            "Application invocation drifted from its session".into(),
                        )));
                    }
                    Ok(Ok(value))
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(invocation_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

/// Cursor replay over the one Applications-owned channel sequence. Workflow
/// and Flow history are deliberately not projected here.
#[derive(Debug, Clone)]
pub struct ReplayApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub after_sequence: u64,
    pub limit: Option<usize>,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ReplayApplicationSession {
    type Output = ApplicationResult<ReplayApplicationSessionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayApplicationSessionResult {
    pub session: ApplicationSession,
    pub messages: Vec<ApplicationMessage>,
    pub current_variables: ConversationVariableRevision,
    pub next_sequence: u64,
    pub has_more: bool,
}

pub struct ReplayApplicationSessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl ReplayApplicationSessionHandler {
    pub fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self { sessions }
    }
}

impl QueryHandler<ReplayApplicationSession> for ReplayApplicationSessionHandler {
    fn execute(
        &self,
        query: ReplayApplicationSession,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ReplayApplicationSessionResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
                query.actor_principal_id,
                &query.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let limit = query
                .limit
                .unwrap_or(DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT);
            if limit == 0 || limit > MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Application message replay limit must be between 1 and {MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT}"
                ))));
            }
            if query.after_sequence > access.session.last_message_sequence {
                return Ok(Err(ApplicationError::Invalid(
                    "Application message replay cursor is beyond the session head".into(),
                )));
            }
            let messages = match sessions
                .list_messages(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.session_id,
                    query.after_sequence,
                    limit,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let mut expected_sequence = query.after_sequence;
            let mut drifted = messages.len() > limit;
            for message in &messages {
                let Some(next_sequence) = expected_sequence.checked_add(1) else {
                    drifted = true;
                    break;
                };
                expected_sequence = next_sequence;
                if message.organization_id != query.organization_id
                    || message.project_id != query.project_id
                    || message.application_id != query.application_id
                    || message.application_release_id != access.session.application_release_id
                    || message.application_release_digest
                        != access.session.application_release_digest
                    || message.session_id != query.session_id
                    || message.sequence != expected_sequence
                    || message.sequence > access.session.last_message_sequence
                    || message.validate().is_err()
                {
                    drifted = true;
                    break;
                }
            }
            if messages.is_empty() && query.after_sequence < access.session.last_message_sequence {
                drifted = true;
            }
            if drifted {
                return Ok(Err(ApplicationError::Internal(
                    "Application message replay repository returned drifted records".into(),
                )));
            }
            let next_sequence = messages
                .last()
                .map_or(query.after_sequence, |message| message.sequence);
            let current_variables =
                match load_current_variables(sessions.as_ref(), &access.session).await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            Ok(Ok(ReplayApplicationSessionResult {
                has_more: next_sequence < access.session.last_message_sequence,
                session: access.session,
                messages,
                current_variables,
                next_sequence,
            }))
        })
    }
}

async fn load_current_variables(
    sessions: &dyn IApplicationSessionRepository,
    session: &ApplicationSession,
) -> ApplicationResult<ConversationVariableRevision> {
    let variables = sessions
        .find_variable_revision(
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
            session.current_variable_revision_id,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::Internal(
                "Application session current variable revision is missing".into(),
            )
        })?;
    variables.validate().map_err(ApplicationError::Internal)?;
    if variables.organization_id != session.organization_id
        || variables.project_id != session.project_id
        || variables.application_id != session.application_id
        || variables.application_release_id != session.application_release_id
        || variables.application_release_digest != session.application_release_digest
        || variables.revision_number != session.current_variable_revision_number
        || variables.values_digest != session.current_variable_digest
        || variables.session_id != session.id
        || variables.created_at > session.updated_at
    {
        return Err(ApplicationError::Internal(
            "Application session current variable revision drifted from its head".into(),
        ));
    }
    Ok(variables)
}
