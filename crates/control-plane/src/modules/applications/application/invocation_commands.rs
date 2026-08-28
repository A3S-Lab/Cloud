use super::delivery_access::project_member_session;
use super::delivery_commands::{RequestApplicationInvocation, RequestApplicationInvocationHandler};
use super::delivery_identity::{idempotency, invocation_id};
use super::resource_access::environment;
use super::{
    ApplicationWorkflowRunEvidence, IApplicationOntologyRevisionPort, IApplicationWorkflowRunPort,
};
use crate::modules::applications::domain::{
    ApplicationInvocation, ApplicationResponseMode, IApplicationRepository,
    IApplicationSessionRepository, APPLICATION_INVOCATION_INPUT_MAX_BYTES,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ApplicationId, ApplicationSessionId, EnvironmentId, OntologyId,
    OntologyRevisionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;

const APPLICATION_INVOCATION_ADMISSION_OVERHEAD_BYTES: usize = 16 * 1024;
const APPLICATION_INVOCATION_ADMISSION_RETRIES: usize = 8;

/// Resolve public delivery authority and submit one exact request to the
/// explicit Applications delivery CQRS contract.
#[derive(Debug, Clone)]
pub struct AdmitApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub environment_id: Option<EnvironmentId>,
    pub response_mode: ApplicationResponseMode,
    pub input: Value,
    pub timeout_seconds: Option<u64>,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
}

impl Command for AdmitApplicationInvocation {
    type Output = ApplicationResult<ApplicationInvocationMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationInvocationMutationResult {
    pub invocation: ApplicationInvocation,
    pub workflow: ApplicationWorkflowRunEvidence,
    pub replayed: bool,
}

pub struct AdmitApplicationInvocationHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
    ontologies: Arc<dyn IApplicationOntologyRevisionPort>,
    environments: Arc<dyn IEnvironmentRepository>,
    workflows: Arc<dyn IApplicationWorkflowRunPort>,
}

impl AdmitApplicationInvocationHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        sessions: Arc<dyn IApplicationSessionRepository>,
        ontologies: Arc<dyn IApplicationOntologyRevisionPort>,
        environments: Arc<dyn IEnvironmentRepository>,
        workflows: Arc<dyn IApplicationWorkflowRunPort>,
    ) -> Self {
        Self {
            applications,
            sessions,
            ontologies,
            environments,
            workflows,
        }
    }
}

impl CommandHandler<AdmitApplicationInvocation> for AdmitApplicationInvocationHandler {
    fn execute(
        &self,
        command: AdmitApplicationInvocation,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationInvocationMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        let ontologies = Arc::clone(&self.ontologies);
        let environments = Arc::clone(&self.environments);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if let Err(error) = project_member_session(
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
                return Ok(Err(error));
            }
            if let Some(environment_id) = command.environment_id {
                if let Err(error) =
                    environment(command.project_id, environment_id, &command.resource_access)
                {
                    return Ok(Err(error));
                }
                match environments
                    .find(command.organization_id, command.project_id, environment_id)
                    .await
                {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(RepositoryError::NotFound) => {
                        return Ok(Err(ApplicationError::NotFound(
                            "Application environment not found".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            let timeout_seconds = match workflows.admit_timeout_seconds(command.timeout_seconds) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let canonical = match canonical_json_bounded(
                &json!({
                    "organizationId": command.organization_id,
                    "projectId": command.project_id,
                    "applicationId": command.application_id,
                    "sessionId": command.session_id,
                    "ontologyId": command.ontology_id,
                    "ontologyRevisionId": command.ontology_revision_id,
                    "environmentId": command.environment_id,
                    "responseMode": command.response_mode,
                    "input": command.input,
                    "timeoutSeconds": timeout_seconds,
                }),
                APPLICATION_INVOCATION_INPUT_MAX_BYTES
                    + APPLICATION_INVOCATION_ADMISSION_OVERHEAD_BYTES,
                "Application invocation admission",
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let request = match idempotency(
                format!(
                    "organizations/{}/projects/{}/applications/{}/sessions/{}/invocations",
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.session_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let ontology = match ontologies
                .resolve_revision(
                    command.organization_id,
                    command.project_id,
                    command.ontology_id,
                    command.ontology_revision_id,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let invocation_id = invocation_id(command.session_id, &request);
            let requested_at = Utc::now();
            let handler = RequestApplicationInvocationHandler::new(
                applications,
                Arc::clone(&sessions),
                workflows,
            );

            for _ in 0..APPLICATION_INVOCATION_ADMISSION_RETRIES {
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
                let expected_session_version = access.session.aggregate_version;
                let effective_requested_at = std::cmp::max(requested_at, access.session.updated_at);
                let result = handler
                    .execute(
                        RequestApplicationInvocation {
                            organization_id: command.organization_id,
                            project_id: command.project_id,
                            application_id: command.application_id,
                            session_id: command.session_id,
                            invocation_id,
                            expected_session_version,
                            response_mode: command.response_mode,
                            input: command.input.clone(),
                            ontology_id: ontology.ontology_id,
                            ontology_revision_id: ontology.ontology_revision_id,
                            ontology_digest: ontology.ontology_digest.clone(),
                            environment_id: command.environment_id,
                            timeout_seconds,
                            actor_principal_id: command.actor_principal_id,
                            resource_access: command.resource_access.clone(),
                            requested_at: effective_requested_at,
                        },
                        context.clone(),
                    )
                    .await?;
                match result {
                    Ok(result) => {
                        return Ok(Ok(ApplicationInvocationMutationResult {
                            invocation: result.invocation,
                            workflow: result.workflow,
                            replayed: result.invocation_replayed,
                        }))
                    }
                    Err(error @ ApplicationError::Conflict(_)) => {
                        match sessions
                            .find_invocation(
                                command.organization_id,
                                command.project_id,
                                command.application_id,
                                invocation_id,
                            )
                            .await
                        {
                            Ok(Some(_)) => return Ok(Err(error)),
                            Ok(None) | Err(RepositoryError::NotFound) => {}
                            Err(read_error) => return Ok(Err(read_error.into())),
                        }
                        let latest = match project_member_session(
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
                            Err(access_error) => return Ok(Err(access_error)),
                        };
                        if latest.session.aggregate_version == expected_session_version {
                            return Ok(Err(error));
                        }
                    }
                    Err(error @ ApplicationError::Invalid(_)) => {
                        let latest = match project_member_session(
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
                            Err(access_error) => return Ok(Err(access_error)),
                        };
                        if latest.session.aggregate_version == expected_session_version {
                            return Ok(Err(error));
                        }
                    }
                    Err(error) => return Ok(Err(error)),
                }
            }
            Ok(Err(ApplicationError::Conflict(
                "Application invocation admission exhausted concurrent session retries".into(),
            )))
        })
    }
}
