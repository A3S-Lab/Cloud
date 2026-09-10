use super::CreateResourceGrant;
use crate::modules::identity::application::{
    IIdentityEnvironmentAccess, IIdentityNodeAccess, IIdentityProjectAccess,
    IdentityEnvironmentScope, IdentityNodeScope, IdentityProjectScope, ResourceGrantMutationResult,
};
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::domain::events::ResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateResourceGrantWrite, IResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ResourceGrantId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateResourceGrantHandler {
    repository: Arc<dyn IResourceGrantRepository>,
    projects: Arc<dyn IIdentityProjectAccess>,
    environments: Arc<dyn IIdentityEnvironmentAccess>,
    nodes: Arc<dyn IIdentityNodeAccess>,
}

impl CreateResourceGrantHandler {
    pub fn new(
        repository: Arc<dyn IResourceGrantRepository>,
        projects: Arc<dyn IIdentityProjectAccess>,
        environments: Arc<dyn IIdentityEnvironmentAccess>,
        nodes: Arc<dyn IIdentityNodeAccess>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::repositories::IResourceGrantRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryIdentityRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, MembershipId, NodeId, OrganizationId, PrincipalId, ProjectId,
        RepositoryError,
    };
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use uuid::Uuid;

    struct PresentProjectAccess;
    struct MissingProjectAccess;
    struct PresentEnvironmentAccess;
    struct MissingEnvironmentAccess;
    struct PresentNodeAccess;
    struct MissingNodeAccess;

    #[async_trait]
    impl IIdentityProjectAccess for PresentProjectAccess {
        async fn project_exists(
            &self,
            _scope: IdentityProjectScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl IIdentityProjectAccess for MissingProjectAccess {
        async fn project_exists(
            &self,
            _scope: IdentityProjectScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl IIdentityEnvironmentAccess for PresentEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl IIdentityEnvironmentAccess for MissingEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl IIdentityNodeAccess for PresentNodeAccess {
        async fn node_exists(&self, _scope: IdentityNodeScope) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl IIdentityNodeAccess for MissingNodeAccess {
        async fn node_exists(&self, _scope: IdentityNodeScope) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    fn command(scope: ResourceGrantScope) -> CreateResourceGrant {
        CreateResourceGrant {
            organization_id: OrganizationId::new(),
            membership_id: MembershipId::new(),
            scope,
            actor_principal_id: PrincipalId::new(),
            idempotency_key: "create-resource-grant".into(),
            request_id: Uuid::now_v7(),
        }
    }

    #[tokio::test]
    async fn missing_project_target_fails_closed_as_not_found() {
        let repository: Arc<dyn IResourceGrantRepository> =
            Arc::new(InMemoryIdentityRepository::new());
        let handler = CreateResourceGrantHandler::new(
            repository,
            Arc::new(MissingProjectAccess),
            Arc::new(PresentEnvironmentAccess),
            Arc::new(PresentNodeAccess),
        );
        let result = handler
            .execute(
                command(ResourceGrantScope::Project {
                    project_id: ProjectId::new(),
                }),
                a3s_boot::CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap();
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn missing_environment_target_fails_closed_as_not_found() {
        let repository: Arc<dyn IResourceGrantRepository> =
            Arc::new(InMemoryIdentityRepository::new());
        let handler = CreateResourceGrantHandler::new(
            repository,
            Arc::new(PresentProjectAccess),
            Arc::new(MissingEnvironmentAccess),
            Arc::new(PresentNodeAccess),
        );
        let result = handler
            .execute(
                command(ResourceGrantScope::Environment {
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                }),
                a3s_boot::CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap();
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn missing_node_target_fails_closed_as_not_found() {
        let repository: Arc<dyn IResourceGrantRepository> =
            Arc::new(InMemoryIdentityRepository::new());
        let handler = CreateResourceGrantHandler::new(
            repository,
            Arc::new(PresentProjectAccess),
            Arc::new(PresentEnvironmentAccess),
            Arc::new(MissingNodeAccess),
        );
        let result = handler
            .execute(
                command(ResourceGrantScope::Node {
                    node_id: NodeId::new(),
                }),
                a3s_boot::CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap();
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }
}
