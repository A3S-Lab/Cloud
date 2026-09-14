use super::delivery_access::anonymous_credential_session;
use super::delivery_commands::OpenApplicationSessionResult;
use super::resource_access::release_not_found;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryCredential, ApplicationEndUser, ApplicationRelease,
    ApplicationSession, ConversationVariableRevision, IApplicationDeliveryCredentialRepository,
    IApplicationRepository, IApplicationSessionRepository, OpenApplicationSessionWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Open an ApplicationSession for an anonymous delivery credential binding.
///
/// Callers supply the Applications-owned opaque lookup key. Identity secret
/// verification is out of scope for `APP0.2-C18`.
#[derive(Debug, Clone)]
pub struct OpenAnonymousApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub session_id: ApplicationSessionId,
    pub credential_lookup_key: String,
    pub initial_variables: Value,
    pub opened_at: DateTime<Utc>,
}

impl Command for OpenAnonymousApplicationSession {
    type Output = ApplicationResult<OpenApplicationSessionResult>;
}

pub struct OpenAnonymousApplicationSessionHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl OpenAnonymousApplicationSessionHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    ) -> Self {
        Self {
            applications,
            sessions,
            credentials,
        }
    }
}

impl CommandHandler<OpenAnonymousApplicationSession> for OpenAnonymousApplicationSessionHandler {
    fn execute(
        &self,
        command: OpenAnonymousApplicationSession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OpenApplicationSessionResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.project_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.application_release_id.as_uuid().is_nil()
                || command.session_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Anonymous Application session request identity is invalid".into(),
                )));
            }
            if let Err(error) =
                ApplicationDeliveryCredential::validate_lookup_key(&command.credential_lookup_key)
            {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let release = match load_release(
                applications.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.application_release_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if release.contract.spec().audience != ApplicationAudience::Anonymous {
                return Ok(Err(ApplicationError::Conflict(
                    "anonymous delivery requires an anonymous Application release".into(),
                )));
            }
            let credential = match credentials
                .find_delivery_credential_by_lookup_key(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    &command.credential_lookup_key,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(credential_not_found()));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            match sessions
                .find_session(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.session_id,
                )
                .await
            {
                Ok(Some(_)) => {
                    return Ok(replay_open_anonymous_session(
                        sessions.as_ref(),
                        &release,
                        &credential,
                        &command,
                    )
                    .await);
                }
                Ok(None) | Err(RepositoryError::NotFound) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let end_user =
                match admit_end_user(&credential, &release, &command, sessions.as_ref()).await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let initial_variables = match ConversationVariableRevision::initial(
                command.session_id,
                &release,
                command.initial_variables.clone(),
                command.opened_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let session = match ApplicationSession::create(
                command.session_id,
                &release,
                &end_user,
                &initial_variables,
                command.opened_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match sessions
                .open_session(OpenApplicationSessionWrite {
                    release: release.clone(),
                    end_user: end_user.clone(),
                    session: session.clone(),
                    initial_variables: initial_variables.clone(),
                })
                .await
            {
                Ok(write) if write.value == session => Ok(Ok(OpenApplicationSessionResult {
                    end_user,
                    session: write.value,
                    initial_variables,
                    replayed: write.replayed,
                })),
                Ok(write) if write.replayed => Ok(replay_open_anonymous_session(
                    sessions.as_ref(),
                    &release,
                    &credential,
                    &command,
                )
                .await),
                Ok(_) => Ok(Err(ApplicationError::Internal(
                    "Application session repository returned drifted anonymous open state".into(),
                ))),
                Err(error) => Ok(recover_open_anonymous_session_after_write_error(
                    sessions.as_ref(),
                    &release,
                    &credential,
                    &command,
                    error,
                )
                .await),
            }
        })
    }
}

async fn admit_end_user(
    credential: &ApplicationDeliveryCredential,
    release: &ApplicationRelease,
    command: &OpenAnonymousApplicationSession,
    sessions: &dyn IApplicationSessionRepository,
) -> ApplicationResult<ApplicationEndUser> {
    let expected = credential
        .admit_anonymous_end_user(release, command.opened_at)
        .map_err(ApplicationError::Conflict)?;
    match sessions
        .find_end_user(
            command.organization_id,
            command.project_id,
            command.application_id,
            expected.id,
        )
        .await
    {
        Ok(Some(value)) => {
            validate_anonymous_end_user(&value, credential, release)?;
            Ok(value)
        }
        Ok(None) | Err(RepositoryError::NotFound) => Ok(expected),
        Err(error) => Err(error.into()),
    }
}

fn validate_anonymous_end_user(
    end_user: &ApplicationEndUser,
    credential: &ApplicationDeliveryCredential,
    release: &ApplicationRelease,
) -> ApplicationResult<()> {
    end_user
        .validate_release(release)
        .map_err(ApplicationError::Conflict)?;
    let expected_id = credential
        .anonymous_end_user_id()
        .map_err(ApplicationError::Invalid)?;
    if end_user.id != expected_id
        || end_user.audience != ApplicationAudience::Anonymous
        || end_user.linked_principal_id.is_some()
        || end_user.created_by != credential.created_by
    {
        return Err(ApplicationError::Conflict(
            "Application end user drifted from its anonymous delivery credential".into(),
        ));
    }
    Ok(())
}

async fn replay_open_anonymous_session(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    credential: &ApplicationDeliveryCredential,
    command: &OpenAnonymousApplicationSession,
) -> ApplicationResult<OpenApplicationSessionResult> {
    // Replay of an already-opened session does not re-require Active status so
    // idempotent retries survive a later disable; foreign credentials still fail.
    let expected_end_user_id = credential
        .anonymous_end_user_id()
        .map_err(ApplicationError::Invalid)?;
    let access = anonymous_credential_session(
        sessions,
        command.organization_id,
        command.project_id,
        command.application_id,
        command.session_id,
        expected_end_user_id,
    )
    .await?;
    validate_anonymous_end_user(&access.end_user, credential, release)?;
    access
        .session
        .validate_release(release)
        .map_err(ApplicationError::Conflict)?;
    let expected = ConversationVariableRevision::initial(
        command.session_id,
        release,
        command.initial_variables.clone(),
        access.session.created_at,
    )
    .map_err(ApplicationError::Invalid)?;
    let initial_variables = sessions
        .find_variable_revision(
            command.organization_id,
            command.project_id,
            command.application_id,
            command.session_id,
            expected.id,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::Internal("Application session initial variables are missing".into())
        })?;
    if initial_variables != expected {
        return Err(ApplicationError::Conflict(
            "Application session identity was reused with different initial variables".into(),
        ));
    }
    Ok(OpenApplicationSessionResult {
        end_user: access.end_user,
        session: access.session,
        initial_variables,
        replayed: true,
    })
}

async fn recover_open_anonymous_session_after_write_error(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    credential: &ApplicationDeliveryCredential,
    command: &OpenAnonymousApplicationSession,
    original_error: RepositoryError,
) -> ApplicationResult<OpenApplicationSessionResult> {
    match replay_open_anonymous_session(sessions, release, credential, command).await {
        Ok(result) => return Ok(result),
        Err(ApplicationError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    let end_user_id = credential
        .anonymous_end_user_id()
        .map_err(ApplicationError::Invalid)?;
    let end_user = match sessions
        .find_end_user(
            command.organization_id,
            command.project_id,
            command.application_id,
            end_user_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(original_error.into()),
        Err(error) => return Err(error.into()),
    };
    validate_anonymous_end_user(&end_user, credential, release)?;
    let opened_at = std::cmp::max(command.opened_at, end_user.created_at);
    let initial_variables = ConversationVariableRevision::initial(
        command.session_id,
        release,
        command.initial_variables.clone(),
        opened_at,
    )
    .map_err(ApplicationError::Invalid)?;
    let session = ApplicationSession::create(
        command.session_id,
        release,
        &end_user,
        &initial_variables,
        opened_at,
    )
    .map_err(ApplicationError::Invalid)?;
    match sessions
        .open_session(OpenApplicationSessionWrite {
            release: release.clone(),
            end_user: end_user.clone(),
            session: session.clone(),
            initial_variables: initial_variables.clone(),
        })
        .await
    {
        Ok(write) if write.value == session => Ok(OpenApplicationSessionResult {
            end_user,
            session: write.value,
            initial_variables,
            replayed: write.replayed,
        }),
        Ok(_) => Err(ApplicationError::Internal(
            "Application session repository returned drifted anonymous recovery state".into(),
        )),
        Err(error) => {
            match replay_open_anonymous_session(sessions, release, credential, command).await {
                Ok(result) => Ok(result),
                Err(ApplicationError::NotFound(_)) => Err(error.into()),
                Err(replay_error) => Err(replay_error),
            }
        }
    }
}

async fn load_release(
    applications: &dyn IApplicationRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    release_id: ApplicationReleaseId,
) -> ApplicationResult<ApplicationRelease> {
    match applications
        .find_release(organization_id, project_id, application_id, release_id)
        .await
    {
        Ok(Some(value)) => Ok(value),
        Ok(None) | Err(RepositoryError::NotFound) => Err(release_not_found()),
        Err(error) => Err(error.into()),
    }
}

fn credential_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application delivery credential not found".into())
}
