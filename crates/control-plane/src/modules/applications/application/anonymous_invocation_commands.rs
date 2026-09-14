use super::anonymous_delivery_commands::{
    load_credential_by_lookup_key, validate_anonymous_end_user,
};
use super::delivery_access::anonymous_credential_session;
use super::delivery_commands::{
    RequestApplicationInvocation, RequestApplicationInvocationResult, load_release,
    same_invocation_request, validate_invocation_replay,
};
use super::{
    ComposeApplicationInvocationWorkflowRun, ComposeApplicationInvocationWorkflowRunHandler,
    IApplicationWorkflowRunPort,
};
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryCredentialStatus, ApplicationInvocation,
    ApplicationInvocationWorkflowAuthority, ApplicationMessage,
    IApplicationDeliveryCredentialRepository, IApplicationRepository,
    IApplicationSessionRepository, RequestApplicationInvocationWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, EnvironmentId, OntologyId,
    OntologyRevisionId, OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Request one ApplicationInvocation for an anonymous delivery credential session.
///
/// Callers supply the Applications-owned opaque lookup key. Identity secret
/// verification is out of scope for `APP0.2-C19`. Workflow authority uses the
/// credential issuer Principal as `requested_by`.
#[derive(Debug, Clone)]
pub struct RequestAnonymousApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub expected_session_version: u64,
    pub credential_lookup_key: String,
    pub response_mode: crate::modules::applications::domain::ApplicationResponseMode,
    pub input: Value,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    pub environment_id: Option<EnvironmentId>,
    pub timeout_seconds: u64,
    pub requested_at: DateTime<Utc>,
}

impl Command for RequestAnonymousApplicationInvocation {
    type Output = ApplicationResult<RequestApplicationInvocationResult>;
}

pub struct RequestAnonymousApplicationInvocationHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl RequestAnonymousApplicationInvocationHandler {
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

impl CommandHandler<RequestAnonymousApplicationInvocation>
    for RequestAnonymousApplicationInvocationHandler
{
    fn execute(
        &self,
        command: RequestAnonymousApplicationInvocation,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RequestApplicationInvocationResult>>,
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
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Anonymous Application invocation request identity is invalid".into(),
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
            let timeout_seconds =
                match workflows.admit_timeout_seconds(Some(command.timeout_seconds)) {
                    Ok(value) if value == command.timeout_seconds => value,
                    Ok(_) => {
                        return Ok(Err(ApplicationError::Conflict(
                            "admitted Application WorkflowRun timeout drifted".into(),
                        )));
                    }
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

            let project_shaped = project_shaped_request(&command, credential.created_by);
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
                    // Replay does not re-require Active so idempotent retries
                    // survive a later disable; foreign credentials still fail.
                    if let Err(error) = validate_invocation_replay(
                        sessions.as_ref(),
                        &release,
                        &access.session,
                        &current,
                        &project_shaped,
                    )
                    .await
                    {
                        return Ok(Err(error));
                    }
                    true
                }
                Ok(None) | Err(RepositoryError::NotFound) => {
                    if credential.status != ApplicationDeliveryCredentialStatus::Active {
                        return Ok(Err(ApplicationError::Conflict(
                            "inactive Application delivery credential cannot request invocations"
                                .into(),
                        )));
                    }
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
                        credential.created_by,
                        timeout_seconds,
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
                                &project_shaped,
                            )
                            .await
                            {
                                return Ok(Err(error));
                            }
                            true
                        }
                        Ok(_) => {
                            return Ok(Err(ApplicationError::Internal(
                                "Application invocation repository returned drifted anonymous request"
                                    .into(),
                            )));
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
                                        &project_shaped,
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

fn project_shaped_request(
    command: &RequestAnonymousApplicationInvocation,
    actor_principal_id: PrincipalId,
) -> RequestApplicationInvocation {
    RequestApplicationInvocation {
        organization_id: command.organization_id,
        project_id: command.project_id,
        application_id: command.application_id,
        session_id: command.session_id,
        invocation_id: command.invocation_id,
        expected_session_version: command.expected_session_version,
        response_mode: command.response_mode,
        input: command.input.clone(),
        ontology_id: command.ontology_id,
        ontology_revision_id: command.ontology_revision_id,
        ontology_digest: command.ontology_digest.clone(),
        environment_id: command.environment_id,
        timeout_seconds: command.timeout_seconds,
        actor_principal_id,
        access: ApplicationAccess::organization_wide(),
        requested_at: command.requested_at,
    }
}
