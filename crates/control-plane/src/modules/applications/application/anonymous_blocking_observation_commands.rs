//! Anonymous-credential CQRS poll for Applications blocking observation.
//!
//! `APP0.2-C48` authorizes one opaque delivery credential, loads the exact
//! anonymous session/invocation, and projects `ApplicationBlockingObservation`
//! without a second run history, migration, streaming cursor, or delivery surface.

use super::anonymous_delivery_commands::load_credential_by_lookup_key;
use super::blocking_observation_commands::load_invocation_messages;
use super::delivery_access::{anonymous_credential_session, invocation_not_found};
use crate::modules::applications::domain::{
    ApplicationBlockingObservation, IApplicationDeliveryCredentialRepository,
    IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_boot::{Query, QueryHandler};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Observe one Blocking-mode invocation for an anonymous delivery credential.
#[derive(Debug, Clone)]
pub struct ObserveAnonymousApplicationBlockingInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub credential_lookup_key: String,
    pub observed_at: DateTime<Utc>,
}

impl Query for ObserveAnonymousApplicationBlockingInvocation {
    type Output = ApplicationResult<ApplicationBlockingObservation>;
}

pub struct ObserveAnonymousApplicationBlockingInvocationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl ObserveAnonymousApplicationBlockingInvocationHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    ) -> Self {
        Self {
            sessions,
            credentials,
        }
    }
}

impl QueryHandler<ObserveAnonymousApplicationBlockingInvocation>
    for ObserveAnonymousApplicationBlockingInvocationHandler
{
    fn execute(
        &self,
        query: ObserveAnonymousApplicationBlockingInvocation,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationBlockingObservation>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            let credential = match load_credential_by_lookup_key(
                credentials.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                &query.credential_lookup_key,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            // Observation reuses an already-admitted session. Do not re-require
            // Active so mid-wait disable still allows poll; foreign keys fail.
            let expected_end_user_id = match credential.anonymous_end_user_id() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let access = match anonymous_credential_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
                expected_end_user_id,
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

            match ApplicationBlockingObservation::observe(
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
