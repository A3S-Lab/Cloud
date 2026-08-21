use super::delivery_access::{invocation_not_found, project_member_session};
use super::resource_access::{project, release_not_found};
use super::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest,
    ComposeApplicationInvocationWorkflowRun, ComposeApplicationInvocationWorkflowRunHandler,
    IApplicationWorkflowRunPort,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, ApplicationAudience, ApplicationEndUser,
    ApplicationInvocation, ApplicationInvocationStatus, ApplicationInvocationWorkflowAuthority,
    ApplicationMessage, ApplicationRelease, ApplicationResponseMode, ApplicationSession,
    ApplicationSessionStatus, CloseApplicationSessionWrite, ConversationVariableRevision,
    IApplicationRepository, IApplicationSessionRepository, OpenApplicationSessionWrite,
    RequestApplicationInvocationWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest,
};
use crate::modules::workflow::domain::workflow_run_timeout_seconds;
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

const APPLICATION_INVOCATION_CANCELLATION_REASON: &str = "Application invocation cancellation";

/// Open one exact-release project-member session.
///
/// `session_id` is the caller's stable replay identity. Reusing it with a
/// different release or initial variable object conflicts.
#[derive(Debug, Clone)]
pub struct OpenApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub session_id: ApplicationSessionId,
    pub initial_variables: Value,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub opened_at: DateTime<Utc>,
}

impl Command for OpenApplicationSession {
    type Output = ApplicationResult<OpenApplicationSessionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApplicationSessionResult {
    pub end_user: ApplicationEndUser,
    pub session: ApplicationSession,
    pub initial_variables: ConversationVariableRevision,
    pub replayed: bool,
}

pub struct OpenApplicationSessionHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl OpenApplicationSessionHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
    ) -> Self {
        Self {
            applications,
            sessions,
        }
    }
}

impl CommandHandler<OpenApplicationSession> for OpenApplicationSessionHandler {
    fn execute(
        &self,
        command: OpenApplicationSession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OpenApplicationSessionResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            if let Err(error) = project(command.project_id, &command.resource_access) {
                return Ok(Err(error));
            }
            if command.organization_id.as_uuid().is_nil()
                || command.project_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.application_release_id.as_uuid().is_nil()
                || command.session_id.as_uuid().is_nil()
                || command.actor_principal_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application session request identity is invalid".into(),
                )));
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
            if release.contract.spec().audience != ApplicationAudience::ProjectMembers {
                return Ok(Err(ApplicationError::Conflict(
                    "project-authorized delivery requires a project-member Application release"
                        .into(),
                )));
            }
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
                    return Ok(replay_open_session(sessions.as_ref(), &release, &command).await)
                }
                Ok(None) | Err(RepositoryError::NotFound) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let end_user_id = match ApplicationEndUser::project_member_id(
                command.application_id,
                command.actor_principal_id,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let end_user = match sessions
                .find_end_user(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    end_user_id,
                )
                .await
            {
                Ok(Some(value)) => {
                    if let Err(error) =
                        value.validate_project_member(&release, command.actor_principal_id)
                    {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                    value
                }
                Ok(None) | Err(RepositoryError::NotFound) => {
                    match ApplicationEndUser::project_member(
                        &release,
                        command.actor_principal_id,
                        command.opened_at,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    }
                }
                Err(error) => return Ok(Err(error.into())),
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
                Ok(write) if write.replayed => {
                    Ok(replay_open_session(sessions.as_ref(), &release, &command).await)
                }
                Ok(_) => Ok(Err(ApplicationError::Internal(
                    "Application session repository returned drifted open state".into(),
                ))),
                Err(error) => Ok(recover_open_session_after_write_error(
                    sessions.as_ref(),
                    &release,
                    &command,
                    error,
                )
                .await),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct CloseApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub closed_at: DateTime<Utc>,
}

impl Command for CloseApplicationSession {
    type Output = ApplicationResult<CloseApplicationSessionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseApplicationSessionResult {
    pub session: ApplicationSession,
    pub replayed: bool,
}

pub struct CloseApplicationSessionHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl CloseApplicationSessionHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
    ) -> Self {
        Self {
            applications,
            sessions,
        }
    }
}

impl CommandHandler<CloseApplicationSession> for CloseApplicationSessionHandler {
    fn execute(
        &self,
        command: CloseApplicationSession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CloseApplicationSessionResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
                command.actor_principal_id,
                &command.resource_access,
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
            if let Err(error) = access
                .end_user
                .validate_project_member(&release, command.actor_principal_id)
                .and_then(|_| access.session.validate_release(&release))
            {
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
                    "Application session repository returned drifted close state".into(),
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

/// Persist one stable invocation request, then start or adopt its sole ordinary
/// WorkflowRun. A failure after admission is repaired by retrying the same
/// `invocation_id` and immutable request.
#[derive(Debug, Clone)]
pub struct RequestApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub expected_session_version: u64,
    pub response_mode: ApplicationResponseMode,
    pub input: Value,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    pub environment_id: Option<EnvironmentId>,
    pub timeout_seconds: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub requested_at: DateTime<Utc>,
}

impl Command for RequestApplicationInvocation {
    type Output = ApplicationResult<RequestApplicationInvocationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestApplicationInvocationResult {
    pub invocation: ApplicationInvocation,
    pub workflow: ApplicationWorkflowRunEvidence,
    pub invocation_replayed: bool,
    pub workflow_replayed: bool,
}

pub struct RequestApplicationInvocationHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl RequestApplicationInvocationHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
        workflows: Arc<dyn IApplicationWorkflowRunPort>,
    ) -> Self {
        Self {
            applications,
            sessions,
            workflows,
        }
    }
}

impl CommandHandler<RequestApplicationInvocation> for RequestApplicationInvocationHandler {
    fn execute(
        &self,
        command: RequestApplicationInvocation,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RequestApplicationInvocationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
                command.actor_principal_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if command.invocation_id.as_uuid().is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "Application invocation identity is invalid".into(),
                )));
            }
            if let Err(error) = workflow_run_timeout_seconds(Some(command.timeout_seconds)) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
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
            if let Err(error) = access
                .end_user
                .validate_project_member(&release, command.actor_principal_id)
                .and_then(|_| access.session.validate_release(&release))
            {
                return Ok(Err(ApplicationError::Conflict(error)));
            }

            let invocation_replayed = match sessions
                .find_invocation(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.invocation_id,
                )
                .await
            {
                Ok(Some(current)) => {
                    if let Err(error) = validate_invocation_replay(
                        sessions.as_ref(),
                        &release,
                        &access.session,
                        &current,
                        &command,
                    )
                    .await
                    {
                        return Ok(Err(error));
                    }
                    true
                }
                Ok(None) | Err(RepositoryError::NotFound) => {
                    let invocation = match ApplicationInvocation::request(
                        command.invocation_id,
                        &access.session,
                        &release,
                        command.response_mode,
                        command.input.clone(),
                        command.requested_at,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    };
                    let workflow_authority = match ApplicationInvocationWorkflowAuthority::new(
                        &invocation,
                        command.ontology_id,
                        command.ontology_revision_id,
                        command.ontology_digest.clone(),
                        command.environment_id,
                        command.actor_principal_id,
                        command.timeout_seconds,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    };
                    let input_message = match ApplicationMessage::input(
                        &access.session,
                        &invocation,
                        invocation.requested_at,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    };
                    match sessions
                        .request_invocation(RequestApplicationInvocationWrite {
                            invocation: invocation.clone(),
                            workflow_authority,
                            input_message,
                            expected_session_version: command.expected_session_version,
                        })
                        .await
                    {
                        Ok(write) if same_invocation_request(&write.value, &invocation) => {
                            write.replayed
                        }
                        Ok(write) if write.replayed => {
                            if let Err(error) = validate_invocation_replay(
                                sessions.as_ref(),
                                &release,
                                &access.session,
                                &write.value,
                                &command,
                            )
                            .await
                            {
                                return Ok(Err(error));
                            }
                            true
                        }
                        Ok(_) => {
                            return Ok(Err(ApplicationError::Internal(
                                "Application invocation repository returned drifted request".into(),
                            )))
                        }
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
                                Ok(Some(current)) => {
                                    if let Err(replay_error) = validate_invocation_replay(
                                        sessions.as_ref(),
                                        &release,
                                        &access.session,
                                        &current,
                                        &command,
                                    )
                                    .await
                                    {
                                        return Ok(Err(replay_error));
                                    }
                                    true
                                }
                                _ => return Ok(Err(error.into())),
                            }
                        }
                    }
                }
                Err(error) => return Ok(Err(error.into())),
            };

            let composition = ComposeApplicationInvocationWorkflowRunHandler::new(
                applications,
                Arc::clone(&sessions),
                workflows,
            )
            .execute(
                ComposeApplicationInvocationWorkflowRun {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    application_id: command.application_id,
                    session_id: command.session_id,
                    invocation_id: command.invocation_id,
                },
                context,
            )
            .await?;
            match composition {
                Ok(value) => Ok(Ok(RequestApplicationInvocationResult {
                    invocation: value.invocation,
                    workflow: value.workflow,
                    invocation_replayed,
                    workflow_replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error)),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct CancelApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelApplicationInvocation {
    type Output = ApplicationResult<CancelApplicationInvocationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelApplicationInvocationResult {
    pub invocation: ApplicationInvocation,
    pub workflow: Option<ApplicationWorkflowRunEvidence>,
    pub replayed: bool,
}

pub struct CancelApplicationInvocationHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl CancelApplicationInvocationHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
        workflows: Arc<dyn IApplicationWorkflowRunPort>,
    ) -> Self {
        Self {
            applications,
            sessions,
            workflows,
        }
    }
}

impl CommandHandler<CancelApplicationInvocation> for CancelApplicationInvocationHandler {
    fn execute(
        &self,
        command: CancelApplicationInvocation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CancelApplicationInvocationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            let access = match project_member_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
                command.actor_principal_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if command.invocation_id.as_uuid().is_nil() || command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "Application cancellation identity or version is invalid".into(),
                )));
            }
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
            if let Err(error) = access
                .end_user
                .validate_project_member(&release, command.actor_principal_id)
                .and_then(|_| access.session.validate_release(&release))
            {
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
                    return Ok(Err(invocation_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let (mut invocation, mut replayed) = match current.status {
                ApplicationInvocationStatus::Succeeded | ApplicationInvocationStatus::Failed => {
                    return Ok(Err(ApplicationError::Conflict(
                        "terminal Application invocation cannot be cancelled".into(),
                    )))
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
                    "bound Application WorkflowRun disappeared during cancellation".into(),
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

async fn replay_open_session(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    command: &OpenApplicationSession,
) -> ApplicationResult<OpenApplicationSessionResult> {
    let access = project_member_session(
        sessions,
        command.organization_id,
        command.project_id,
        command.application_id,
        command.session_id,
        command.actor_principal_id,
        &command.resource_access,
    )
    .await?;
    access
        .end_user
        .validate_project_member(release, command.actor_principal_id)
        .and_then(|_| access.session.validate_release(release))
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

/// Resolve an ambiguous open commit and the race where another session first
/// creates this Principal's deterministic Application end user.
async fn recover_open_session_after_write_error(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    command: &OpenApplicationSession,
    original_error: RepositoryError,
) -> ApplicationResult<OpenApplicationSessionResult> {
    match replay_open_session(sessions, release, command).await {
        Ok(result) => return Ok(result),
        Err(ApplicationError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    let end_user_id =
        ApplicationEndUser::project_member_id(command.application_id, command.actor_principal_id)
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
    end_user
        .validate_project_member(release, command.actor_principal_id)
        .map_err(ApplicationError::Conflict)?;
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
            "Application session repository returned drifted recovery state".into(),
        )),
        Err(error) => match replay_open_session(sessions, release, command).await {
            Ok(result) => Ok(result),
            Err(ApplicationError::NotFound(_)) => Err(error.into()),
            Err(replay_error) => Err(replay_error),
        },
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

async fn validate_invocation_replay(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    session: &ApplicationSession,
    current: &ApplicationInvocation,
    command: &RequestApplicationInvocation,
) -> ApplicationResult<()> {
    if current.session_id != command.session_id {
        return Err(invocation_not_found());
    }
    let expected = ApplicationInvocation::request(
        command.invocation_id,
        session,
        release,
        command.response_mode,
        command.input.clone(),
        current.requested_at,
    )
    .map_err(ApplicationError::Invalid)?;
    let authority = sessions
        .find_invocation_workflow_authority(
            command.organization_id,
            command.project_id,
            command.application_id,
            command.invocation_id,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::Internal(
                "Application invocation Workflow authority is missing".into(),
            )
        })?;
    let expected_authority = ApplicationInvocationWorkflowAuthority::new(
        current,
        command.ontology_id,
        command.ontology_revision_id,
        command.ontology_digest.clone(),
        command.environment_id,
        command.actor_principal_id,
        command.timeout_seconds,
    )
    .map_err(ApplicationError::Invalid)?;
    if !same_invocation_request(current, &expected) || authority != expected_authority {
        return Err(ApplicationError::Conflict(
            "Application invocation identity was reused with different input or execution authority"
                .into(),
        ));
    }
    Ok(())
}

fn same_invocation_request(
    current: &ApplicationInvocation,
    expected: &ApplicationInvocation,
) -> bool {
    current.organization_id == expected.organization_id
        && current.project_id == expected.project_id
        && current.application_id == expected.application_id
        && current.application_release_id == expected.application_release_id
        && current.application_release_digest == expected.application_release_digest
        && current.session_id == expected.session_id
        && current.id == expected.id
        && current.response_mode == expected.response_mode
        && current.input == expected.input
        && current.input_digest == expected.input_digest
        && current.requested_at == expected.requested_at
}

async fn load_workflow_request(
    sessions: &dyn IApplicationSessionRepository,
    release: &ApplicationRelease,
    session: &ApplicationSession,
    invocation: &ApplicationInvocation,
) -> ApplicationResult<ApplicationWorkflowRunRequest> {
    let authority = sessions
        .find_invocation_workflow_authority(
            invocation.organization_id,
            invocation.project_id,
            invocation.application_id,
            invocation.id,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::Conflict(
                "Application invocation Workflow authority is missing".into(),
            )
        })?;
    ApplicationWorkflowRunRequest::from_invocation(release, session, invocation, &authority)
        .map_err(ApplicationError::Conflict)
}
