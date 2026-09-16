//! Anonymous-credential CQRS for Application session close and invocation cancel.
//!
//! `APP0.2-C55` authorizes one opaque delivery credential, loads the exact
//! anonymous session (and invocation for cancel), and reuses the member close /
//! cancel durability rules without project-member access or a second authority.
//! Already-admitted sessions remain closable/cancellable after credential disable;
//! foreign lookup keys still fail closed.

use super::anonymous_delivery_commands::{
    load_credential_by_lookup_key, validate_anonymous_end_user,
};
use super::delivery_access::{anonymous_credential_session, invocation_not_found};
use super::delivery_commands::{
    load_release, load_workflow_request, CancelApplicationInvocationResult,
    CloseApplicationSessionResult, APPLICATION_INVOCATION_CANCELLATION_REASON,
};
use super::IApplicationWorkflowRunPort;
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, ApplicationAudience, ApplicationInvocationStatus,
    ApplicationSessionStatus, CloseApplicationSessionWrite,
    IApplicationDeliveryCredentialRepository, IApplicationRepository,
    IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Close one ApplicationSession for an anonymous delivery credential.
///
/// Callers supply the Applications-owned opaque lookup key. Close does not
/// re-require Active so an already-admitted session can shut down after disable.
#[derive(Debug, Clone)]
pub struct CloseAnonymousApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub expected_version: u64,
    pub credential_lookup_key: String,
    pub closed_at: DateTime<Utc>,
}

impl Command for CloseAnonymousApplicationSession {
    type Output = ApplicationResult<CloseApplicationSessionResult>;
}

pub struct CloseAnonymousApplicationSessionHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl CloseAnonymousApplicationSessionHandler {
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

impl CommandHandler<CloseAnonymousApplicationSession>
    for CloseAnonymousApplicationSessionHandler
{
    fn execute(
        &self,
        command: CloseAnonymousApplicationSession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CloseApplicationSessionResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.project_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.session_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Anonymous Application session close identity is invalid".into(),
                )));
            }
            let credential = match load_credential_by_lookup_key(
                credentials.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                &command.credential_lookup_key,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            // Close reuses an already-admitted session. Do not re-require Active
            // so mid-flight disable still allows shutdown; foreign keys fail.
            let expected_end_user_id = match credential.anonymous_end_user_id() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let access = match anonymous_credential_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
                expected_end_user_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let release = match load_release(
                applications.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                access.session.application_release_id,
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
            if let Err(error) = validate_anonymous_end_user(&access.end_user, &credential, &release)
            {
                return Ok(Err(error));
            }
            if let Err(error) = access.session.validate_release(&release) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            if access.session.status == ApplicationSessionStatus::Closed {
                if access.session.aggregate_version != command.expected_version.saturating_add(1) {
                    return Ok(Err(ApplicationError::Conflict(
                        "Application session close replay used a different version".into(),
                    )));
                }
                return Ok(Ok(CloseApplicationSessionResult {
                    session: access.session,
                    replayed: true,
                }));
            }
            let closed = match access
                .session
                .close(command.expected_version, command.closed_at)
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            match sessions
                .close_session(CloseApplicationSessionWrite {
                    session: closed.clone(),
                    expected_version: command.expected_version,
                })
                .await
            {
                Ok(write) if write.value == closed => Ok(Ok(CloseApplicationSessionResult {
                    session: write.value,
                    replayed: write.replayed,
                })),
                Ok(_) => Ok(Err(ApplicationError::Internal(
                    "Application session repository returned drifted anonymous close state".into(),
                ))),
                Err(error) => {
                    let recovered = sessions
                        .find_session(
                            command.organization_id,
                            command.project_id,
                            command.application_id,
                            command.session_id,
                        )
                        .await;
                    match recovered {
                        Ok(Some(current))
                            if current.status == ApplicationSessionStatus::Closed
                                && current.aggregate_version
                                    == command.expected_version.saturating_add(1)
                                && current.validate_release(&release).is_ok() =>
                        {
                            Ok(Ok(CloseApplicationSessionResult {
                                session: current,
                                replayed: true,
                            }))
                        }
                        _ => Ok(Err(error.into())),
                    }
                }
            }
        })
    }
}

/// Cancel one ApplicationInvocation for an anonymous delivery credential session.
///
/// Callers supply the Applications-owned opaque lookup key. Cancel does not
/// re-require Active so an already-admitted invocation can stop after disable.
#[derive(Debug, Clone)]
pub struct CancelAnonymousApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub expected_version: u64,
    pub credential_lookup_key: String,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelAnonymousApplicationInvocation {
    type Output = ApplicationResult<CancelApplicationInvocationResult>;
}

pub struct CancelAnonymousApplicationInvocationHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl CancelAnonymousApplicationInvocationHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
        workflows: Arc<dyn IApplicationWorkflowRunPort>,
    ) -> Self {
        Self {
            applications,
            sessions,
            credentials,
            workflows,
        }
    }
}

impl CommandHandler<CancelAnonymousApplicationInvocation>
    for CancelAnonymousApplicationInvocationHandler
{
    fn execute(
        &self,
        command: CancelAnonymousApplicationInvocation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CancelApplicationInvocationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let credentials = Arc::clone(&self.credentials);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.project_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.session_id.as_uuid().is_nil()
                || command.invocation_id.as_uuid().is_nil()
                || command.expected_version == 0
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Anonymous Application cancellation identity or version is invalid".into(),
                )));
            }
            let credential = match load_credential_by_lookup_key(
                credentials.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                &command.credential_lookup_key,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            // Cancel reuses an already-admitted session/invocation. Do not
            // re-require Active so mid-flight disable still allows stop; foreign
            // keys fail.
            let expected_end_user_id = match credential.anonymous_end_user_id() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let access = match anonymous_credential_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
                expected_end_user_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let release = match load_release(
                applications.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                access.session.application_release_id,
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
            if let Err(error) = validate_anonymous_end_user(&access.end_user, &credential, &release)
            {
                return Ok(Err(error));
            }
            if let Err(error) = access.session.validate_release(&release) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let current = match sessions
                .find_invocation(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.invocation_id,
                )
                .await
            {
                Ok(Some(value)) if value.session_id == command.session_id => value,
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(invocation_not_found()));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let (mut invocation, mut replayed) = match current.status {
                ApplicationInvocationStatus::Succeeded | ApplicationInvocationStatus::Failed => {
                    return Ok(Err(ApplicationError::Conflict(
                        "terminal Application invocation cannot be cancelled".into(),
                    )));
                }
                ApplicationInvocationStatus::Cancelled => {
                    let first_successor = command.expected_version.saturating_add(1);
                    let second_successor = command.expected_version.saturating_add(2);
                    if current.aggregate_version != first_successor
                        && current.aggregate_version != second_successor
                    {
                        return Ok(Err(ApplicationError::Conflict(
                            "Application invocation cancellation replay used a different version"
                                .into(),
                        )));
                    }
                    (current, true)
                }
                ApplicationInvocationStatus::Cancelling => {
                    if current.aggregate_version != command.expected_version.saturating_add(1) {
                        return Ok(Err(ApplicationError::Conflict(
                            "Application invocation cancellation replay used a different version"
                                .into(),
                        )));
                    }
                    (current, true)
                }
                ApplicationInvocationStatus::Requested | ApplicationInvocationStatus::Running => {
                    let cancelling = match current
                        .request_cancellation(command.expected_version, command.requested_at)
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
                    };
                    match sessions
                        .advance_invocation(AdvanceApplicationInvocationWrite {
                            invocation: cancelling.clone(),
                            expected_version: command.expected_version,
                        })
                        .await
                    {
                        Ok(write) => (write.value, write.replayed),
                        Err(error) => {
                            let recovered = sessions
                                .find_invocation(
                                    command.organization_id,
                                    command.project_id,
                                    command.application_id,
                                    command.invocation_id,
                                )
                                .await;
                            match recovered {
                                Ok(Some(value))
                                    if value.session_id == command.session_id
                                        && matches!(
                                            value.status,
                                            ApplicationInvocationStatus::Cancelling
                                                | ApplicationInvocationStatus::Cancelled
                                        ) =>
                                {
                                    (value, true)
                                }
                                _ => return Ok(Err(error.into())),
                            }
                        }
                    }
                }
            };

            let request = match load_workflow_request(
                sessions.as_ref(),
                &release,
                &access.session,
                &invocation,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let cancellation_requested_at =
                std::cmp::max(command.requested_at, invocation.updated_at);
            let workflow = match workflows
                .request_cancellation(
                    &request,
                    APPLICATION_INVOCATION_CANCELLATION_REASON,
                    cancellation_requested_at,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if let Some(evidence) = &workflow {
                if let Err(error) = evidence.validate_against(&request) {
                    return Ok(Err(ApplicationError::Internal(error)));
                }
            } else if invocation.workflow_run_id.is_some() {
                return Ok(Err(ApplicationError::Internal(
                    "bound Application WorkflowRun disappeared during anonymous cancellation"
                        .into(),
                )));
            }

            if workflow.is_none()
                && invocation.status == ApplicationInvocationStatus::Cancelling
                && invocation.workflow_run_id.is_none()
            {
                let cancelled = match invocation.observe_terminal(
                    invocation.aggregate_version,
                    ApplicationInvocationStatus::Cancelled,
                    cancellation_requested_at,
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
                };
                match sessions
                    .advance_invocation(AdvanceApplicationInvocationWrite {
                        invocation: cancelled.clone(),
                        expected_version: invocation.aggregate_version,
                    })
                    .await
                {
                    Ok(write) => {
                        invocation = write.value;
                        replayed |= write.replayed;
                    }
                    Err(error) => {
                        match sessions
                            .find_invocation(
                                command.organization_id,
                                command.project_id,
                                command.application_id,
                                command.invocation_id,
                            )
                            .await
                        {
                            Ok(Some(value))
                                if value.session_id == command.session_id
                                    && value.status == ApplicationInvocationStatus::Cancelled =>
                            {
                                invocation = value;
                                replayed = true;
                            }
                            _ => return Ok(Err(error.into())),
                        }
                    }
                }
            }
            Ok(Ok(CancelApplicationInvocationResult {
                invocation,
                workflow,
                replayed,
            }))
        })
    }
}
