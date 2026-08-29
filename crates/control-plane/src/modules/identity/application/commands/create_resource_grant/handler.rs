use super::CreateResourceGrant;
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::identity::application::ResourceGrantMutationResult;
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::domain::events::ResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateResourceGrantWrite, IResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError, ResourceGrantId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateResourceGrantHandler {
    repository: Arc<dyn IResourceGrantRepository>,
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
    nodes: Arc<dyn INodeRepository>,
}

impl CreateResourceGrantHandler {
    pub fn new(
        repository: Arc<dyn IResourceGrantRepository>,
        projects: Arc<dyn IProjectRepository>,
        environments: Arc<dyn IEnvironmentRepository>,
        nodes: Arc<dyn INodeRepository>,
    ) -> Self {
        Self {
            repository,
            projects,
            environments,
            nodes,
        }
    }
}

impl CommandHandler<CreateResourceGrant> for CreateResourceGrantHandler {
    fn execute(
        &self,
        command: CreateResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ResourceGrantMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        let projects = Arc::clone(&self.projects);
        let environments = Arc::clone(&self.environments);
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            let target_exists = match command.scope {
                ResourceGrantScope::Project { project_id } => {
                    match projects.find(command.organization_id, project_id).await {
                        Ok(project) => project.is_some(),
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => {
                    match environments
                        .find(command.organization_id, project_id, environment_id)
                        .await
                    {
                        Ok(environment) => environment.is_some(),
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                ResourceGrantScope::Node { node_id } => {
                    match nodes.find(command.organization_id, node_id).await {
                        Ok(_) => true,
                        Err(RepositoryError::NotFound) => false,
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
            };
            if !target_exists {
                return Ok(Err(ApplicationError::NotFound(
                    "Resource Grant target not found in organization".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "membershipId": command.membership_id,
                "scope": command.scope,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/memberships/{}/resource-grants",
                    command.organization_id, command.membership_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let grant = ResourceGrant::create(
                ResourceGrantId::new(),
                command.organization_id,
                command.membership_id,
                command.scope,
                Utc::now(),
            );
            let event = ResourceGrantChanged::created(&grant, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create_resource_grant(CreateResourceGrantWrite {
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
            Ok(Ok(ResourceGrantMutationResult {
                resource_grant: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
