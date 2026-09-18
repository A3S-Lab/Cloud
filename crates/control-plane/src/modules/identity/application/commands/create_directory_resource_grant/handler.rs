use super::CreateDirectoryResourceGrant;
use crate::modules::identity::application::{
    IIdentityApplicationAccess, IIdentityEnvironmentAccess, IIdentityNodeAccess,
    IIdentityProjectAccess, IdentityApplicationScope, IdentityEnvironmentScope, IdentityNodeScope,
    IdentityProjectScope, DirectoryResourceGrantMutationResult,
};
use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::domain::events::DirectoryResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateDirectoryResourceGrantWrite, IDirectoryResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectRef, ResourceGrantScope,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ResourceGrantId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateDirectoryResourceGrantHandler {
    repository: Arc<dyn IDirectoryResourceGrantRepository>,
    projects: Arc<dyn IIdentityProjectAccess>,
    environments: Arc<dyn IIdentityEnvironmentAccess>,
    applications: Arc<dyn IIdentityApplicationAccess>,
    nodes: Arc<dyn IIdentityNodeAccess>,
}

impl CreateDirectoryResourceGrantHandler {
    pub fn new(
        repository: Arc<dyn IDirectoryResourceGrantRepository>,
        projects: Arc<dyn IIdentityProjectAccess>,
        environments: Arc<dyn IIdentityEnvironmentAccess>,
        applications: Arc<dyn IIdentityApplicationAccess>,
        nodes: Arc<dyn IIdentityNodeAccess>,
    ) -> Self {
        Self {
            repository,
            projects,
            environments,
            applications,
            nodes,
        }
    }
}

impl CommandHandler<CreateDirectoryResourceGrant> for CreateDirectoryResourceGrantHandler {
    fn execute(
        &self,
        command: CreateDirectoryResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DirectoryResourceGrantMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        let projects = Arc::clone(&self.projects);
        let environments = Arc::clone(&self.environments);
        let applications = Arc::clone(&self.applications);
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            let subject = match DirectoryGrantSubjectRef::parse(command.subject_ref) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let target_exists = match command.scope {
                ResourceGrantScope::Project { project_id } => {
                    let scope = match IdentityProjectScope::new(command.organization_id, project_id)
                    {
                        Ok(scope) => scope,
                        Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
                    };
                    match projects.project_exists(scope).await {
                        Ok(exists) => exists,
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => {
                    let scope = match IdentityEnvironmentScope::new(
                        command.organization_id,
                        project_id,
                        environment_id,
                    ) {
                        Ok(scope) => scope,
                        Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
                    };
                    match environments.environment_exists(scope).await {
                        Ok(exists) => exists,
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                ResourceGrantScope::Application {
                    project_id,
                    application_id,
                } => {
                    let scope = match IdentityApplicationScope::new(
                        command.organization_id,
                        project_id,
                        application_id,
                    ) {
                        Ok(scope) => scope,
                        Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
                    };
                    match applications.application_exists(scope).await {
                        Ok(exists) => exists,
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                ResourceGrantScope::Node { node_id } => {
                    let scope = match IdentityNodeScope::new(command.organization_id, node_id) {
                        Ok(scope) => scope,
                        Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
                    };
                    match nodes.node_exists(scope).await {
                        Ok(exists) => exists,
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
            };
            if !target_exists {
                return Ok(Err(ApplicationError::NotFound(
                    "Directory Resource Grant target not found in organization".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "subjectRef": subject.format_ref(),
                "scope": command.scope,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/directory-resource-grants",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let grant = DirectoryResourceGrant::create(
                ResourceGrantId::new(),
                command.organization_id,
                subject,
                command.scope,
                Utc::now(),
            );
            let event = DirectoryResourceGrantChanged::created(&grant, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create_directory_resource_grant(CreateDirectoryResourceGrantWrite {
                    grant,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(DirectoryResourceGrantMutationResult {
                directory_resource_grant: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
