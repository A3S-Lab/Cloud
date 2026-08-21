use super::delivery_commands::{OpenApplicationSession, OpenApplicationSessionHandler};
use super::delivery_identity::{idempotency, session_id};
use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationSession, IApplicationRepository, IApplicationSessionRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId,
    ProjectId,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;

const APPLICATION_SESSION_ADMISSION_MAX_BYTES: usize = 300 * 1024;

/// Translate the public idempotency contract into the explicit delivery CQRS
/// identity used by the Applications core.
#[derive(Debug, Clone)]
pub struct AdmitApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub release_id: ApplicationReleaseId,
    pub initial_variables: Value,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
}

impl Command for AdmitApplicationSession {
    type Output = ApplicationResult<ApplicationSessionMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationSessionMutationResult {
    pub session: ApplicationSession,
    pub replayed: bool,
}

pub struct AdmitApplicationSessionHandler {
    applications: Arc<dyn IApplicationRepository>,
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl AdmitApplicationSessionHandler {
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

impl CommandHandler<AdmitApplicationSession> for AdmitApplicationSessionHandler {
    fn execute(
        &self,
        command: AdmitApplicationSession,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationSessionMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let canonical = match canonical_json_bounded(
                &json!({
                    "organizationId": command.organization_id,
                    "projectId": command.project_id,
                    "applicationId": command.application_id,
                    "releaseId": command.release_id,
                    "initialVariables": command.initial_variables,
                }),
                APPLICATION_SESSION_ADMISSION_MAX_BYTES,
                "Application session admission",
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let request = match idempotency(
                format!(
                    "organizations/{}/projects/{}/applications/{}/sessions",
                    command.organization_id, command.project_id, command.application_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let end_user_id = match ApplicationEndUser::project_member_id(
                command.application_id,
                command.actor_principal_id,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = OpenApplicationSessionHandler::new(applications, sessions)
                .execute(
                    OpenApplicationSession {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        application_id: command.application_id,
                        application_release_id: command.release_id,
                        session_id: session_id(end_user_id, &request),
                        initial_variables: command.initial_variables,
                        actor_principal_id: command.actor_principal_id,
                        resource_access: command.resource_access,
                        opened_at: Utc::now(),
                    },
                    context,
                )
                .await?;
            Ok(result.map(|result| ApplicationSessionMutationResult {
                session: result.session,
                replayed: result.replayed,
            }))
        })
    }
}
