use super::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest, IApplicationWorkflowRunPort,
};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, ApplicationInvocation, ApplicationInvocationStatus,
    ApplicationSessionStatus, IApplicationRepository, IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, EnvironmentId, OntologyId,
    OntologyRevisionId, OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ComposeApplicationInvocationWorkflowRun {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    pub environment_id: Option<EnvironmentId>,
    pub requested_by: PrincipalId,
    pub timeout_seconds: u64,
}

impl Command for ComposeApplicationInvocationWorkflowRun {
    type Output = ApplicationResult<ComposeApplicationInvocationWorkflowRunResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeApplicationInvocationWorkflowRunResult {
    pub invocation: ApplicationInvocation,
    pub workflow: ApplicationWorkflowRunEvidence,
    pub replayed: bool,
}

pub struct ComposeApplicationInvocationWorkflowRunHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl ComposeApplicationInvocationWorkflowRunHandler {
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

impl CommandHandler<ComposeApplicationInvocationWorkflowRun>
    for ComposeApplicationInvocationWorkflowRunHandler
{
    fn execute(
        &self,
        command: ComposeApplicationInvocationWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ComposeApplicationInvocationWorkflowRunResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.project_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.session_id.as_uuid().is_nil()
                || command.invocation_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application WorkflowRun composition scope is invalid".into(),
                )));
            }
            let session = match sessions
                .find_session(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.session_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Application session not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let release = match applications
                .find_release(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    session.application_release_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Application release not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let invocation = match sessions
                .find_invocation(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.invocation_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Application invocation not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if invocation.session_id != session.id {
                return Ok(Err(ApplicationError::NotFound(
                    "Application invocation not found in session".into(),
                )));
            }
            if invocation.status == ApplicationInvocationStatus::Requested
                && session.status != ApplicationSessionStatus::Active
            {
                return Ok(Err(ApplicationError::Conflict(
                    "closed Application session cannot compose a WorkflowRun".into(),
                )));
            }
            let request = match ApplicationWorkflowRunRequest::from_invocation(
                &release,
                &session,
                &invocation,
                command.ontology_id,
                command.ontology_revision_id,
                command.ontology_digest,
                command.environment_id,
                command.requested_by,
                command.timeout_seconds,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let expected_run_id = request.workflow_run_id();
            if invocation
                .workflow_run_id
                .is_some_and(|workflow_run_id| workflow_run_id != expected_run_id)
            {
                return Ok(Err(ApplicationError::Conflict(
                    "Application invocation is bound to a different WorkflowRun".into(),
                )));
            }
            if invocation.workflow_run_id.is_none()
                && invocation.status != ApplicationInvocationStatus::Requested
            {
                return Ok(Err(ApplicationError::Conflict(
                    "cancelled Application invocation cannot start a WorkflowRun".into(),
                )));
            }
            let evidence = match workflows.start_or_adopt(&request).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if let Err(error) = evidence.validate_against(&request) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            if invocation.workflow_run_id == Some(expected_run_id) {
                return Ok(Ok(ComposeApplicationInvocationWorkflowRunResult {
                    invocation,
                    workflow: evidence,
                    replayed: true,
                }));
            }
            let running = match invocation.bind_workflow_run(
                invocation.aggregate_version,
                expected_run_id,
                invocation.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let write = AdvanceApplicationInvocationWrite {
                invocation: running.clone(),
                expected_version: invocation.aggregate_version,
            };
            match sessions.advance_invocation(write).await {
                Ok(result) if result.value == running => {
                    Ok(Ok(ComposeApplicationInvocationWorkflowRunResult {
                        invocation: result.value,
                        workflow: evidence,
                        replayed: result.replayed,
                    }))
                }
                Ok(_) => Ok(Err(ApplicationError::Internal(
                    "Application invocation binding repository returned drifted state".into(),
                ))),
                Err(error) => {
                    let current = sessions
                        .find_invocation(
                            command.organization_id,
                            command.project_id,
                            command.application_id,
                            command.invocation_id,
                        )
                        .await;
                    match current {
                        Ok(Some(current))
                            if current.workflow_run_id == Some(expected_run_id) =>
                        {
                            Ok(Ok(ComposeApplicationInvocationWorkflowRunResult {
                                invocation: current,
                                workflow: evidence,
                                replayed: true,
                            }))
                        }
                        Ok(Some(current))
                            if current.workflow_run_id.is_none()
                                && matches!(
                                    current.status,
                                    ApplicationInvocationStatus::Cancelling
                                        | ApplicationInvocationStatus::Cancelled
                                ) =>
                        {
                            let cancellation = workflows
                                .request_cancellation(
                                    &request,
                                    "Application invocation was cancelled before WorkflowRun binding",
                                    current.updated_at,
                                )
                                .await;
                            match cancellation {
                                Ok(Some(cancelled)) => {
                                    if let Err(drift) = cancelled.validate_against(&request) {
                                        return Ok(Err(ApplicationError::Internal(drift)));
                                    }
                                    Ok(Err(ApplicationError::Conflict(
                                        "Application invocation was cancelled before WorkflowRun binding"
                                            .into(),
                                    )))
                                }
                                Ok(None) => Ok(Err(ApplicationError::Internal(
                                    "Application WorkflowRun disappeared during cancellation recovery"
                                        .into(),
                                ))),
                                Err(cancellation_error) => Ok(Err(ApplicationError::Unavailable(
                                    format!(
                                        "Application invocation binding failed ({error}); WorkflowRun cancellation recovery failed: {cancellation_error}"
                                    ),
                                ))),
                            }
                        }
                        Ok(_) => Ok(Err(error.into())),
                        Err(read_error) => Ok(Err(ApplicationError::Unavailable(format!(
                            "Application invocation binding failed ({error}); recovery read failed: {read_error}"
                        )))),
                    }
                }
            }
        })
    }
}
