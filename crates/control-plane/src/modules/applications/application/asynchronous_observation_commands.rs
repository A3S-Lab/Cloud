//! Project-authorized CQRS poll for Applications asynchronous observation.
//!
//! `APP0.2-C46` authorizes one exact session/invocation, pages the existing
//! Applications-owned message sequence, and projects `ApplicationAsynchronousObservation`
//! without a second run history, migration, streaming cursor, or public delivery surface.
//! Management delivery is `APP0.2-C47`.

use super::delivery_access::{invocation_not_found, project_member_session};
use super::delivery_queries::MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationAsynchronousObservation, ApplicationMessage, IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, PrincipalId,
    ProjectId, RepositoryError,
};
use a3s_boot::{Query, QueryHandler};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Observe one Asynchronous-mode invocation against its session message frames.
#[derive(Debug, Clone)]
pub struct ObserveApplicationAsynchronousInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub observed_at: DateTime<Utc>,
}

impl Query for ObserveApplicationAsynchronousInvocation {
    type Output = ApplicationResult<ApplicationAsynchronousObservation>;
}

pub struct ObserveApplicationAsynchronousInvocationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl ObserveApplicationAsynchronousInvocationHandler {
    pub fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self { sessions }
    }
}

impl QueryHandler<ObserveApplicationAsynchronousInvocation>
    for ObserveApplicationAsynchronousInvocationHandler
{
    fn execute(
        &self,
        query: ObserveApplicationAsynchronousInvocation,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationAsynchronousObservation>>,
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
                &query.access,
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
            let invocation = match sessions
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
                    value
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(invocation_not_found()));
                }
                Err(error) => return Ok(Err(error.into())),
            };

            let messages = match load_invocation_messages(
                sessions.as_ref(),
                &access.session,
                query.invocation_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };

            match ApplicationAsynchronousObservation::observe(
                &access.session,
                &invocation,
                &messages,
                query.observed_at,
            ) {
                Ok(observation) => Ok(Ok(observation)),
                Err(error) => Ok(Err(ApplicationError::Invalid(error))),
            }
        })
    }
}

async fn load_invocation_messages(
    sessions: &dyn IApplicationSessionRepository,
    session: &crate::modules::applications::domain::ApplicationSession,
    invocation_id: ApplicationInvocationId,
) -> ApplicationResult<Vec<ApplicationMessage>> {
    let mut after_sequence = 0u64;
    let mut collected = Vec::new();
    while after_sequence < session.last_message_sequence {
        let page = match sessions
            .list_messages(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
                after_sequence,
                MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
            )
            .await
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        if page.is_empty() {
            break;
        }
        for message in page {
            if message.sequence <= after_sequence {
                return Err(ApplicationError::Internal(
                    "Application message replay cursor did not advance".into(),
                ));
            }
            after_sequence = message.sequence;
            if message.invocation_id == invocation_id {
                collected.push(message);
            }
        }
    }
    Ok(collected)
}
