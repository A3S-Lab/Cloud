use super::{CreateGithubRepositorySubscription, CreateGithubRepositorySubscriptionResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, SourceSubscriptionId};
use crate::modules::sources::application::{
    ISourceEnvironmentAccess,
    commands::resolve_external_source_revision::DockerfileBuildRecipeInput,
};
use crate::modules::sources::domain::{
    BuildRecipe, CreateGithubRepositorySubscription as PersistGithubRepositorySubscription,
    GitProvider, GitReference, GitRepository, GithubRepositorySubscription,
    GithubRepositorySubscriptionCreated, IGithubConnectionRepository,
    ISourceSubscriptionRepository, NewGithubRepositorySubscription, SourceRepositoryPolicy,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct CreateGithubRepositorySubscriptionHandler {
    environment_access: Arc<dyn ISourceEnvironmentAccess>,
    connections: Arc<dyn IGithubConnectionRepository>,
    subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    policy: Arc<SourceRepositoryPolicy>,
}

impl CreateGithubRepositorySubscriptionHandler {
    pub(in crate::modules::sources) fn from_environment_access(
        environment_access: Arc<dyn ISourceEnvironmentAccess>,
        connections: Arc<dyn IGithubConnectionRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self {
            environment_access,
            connections,
            subscriptions,
            policy,
        }
    }
}

impl CommandHandler<CreateGithubRepositorySubscription>
    for CreateGithubRepositorySubscriptionHandler
{
    fn execute(
        &self,
        command: CreateGithubRepositorySubscription,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateGithubRepositorySubscriptionResult>>,
    > {
        let environment_access = Arc::clone(&self.environment_access);
        let connections = Arc::clone(&self.connections);
        let subscriptions = Arc::clone(&self.subscriptions);
        let policy = Arc::clone(&self.policy);
        Box::pin(async move {
            if !command
                .access
                .environment_is_visible(command.project_id, command.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "source subscriptions not found".into(),
                )));
            }
            if let Err(error) = environment_access
                .require_environment(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                return Ok(Err(error));
            }
            let connection = match connections.find(command.organization_id).await {
                Ok(Some(value)) if value.is_authoritative() => value,
                Ok(Some(_)) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "GitHub source connection is not active".into(),
                    )));
                }
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "verified GitHub source connection not found for organization".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let provider = match GitProvider::parse(&command.repository_provider) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let repository = match GitRepository::parse(provider, &command.repository_url) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = policy.require(&repository) {
                return Ok(Err(ApplicationError::Forbidden(error)));
            }
            let branch = match GitReference::parse("branch", command.branch) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let DockerfileBuildRecipeInput {
                schema,
                kind,
                context_path,
                dockerfile_path,
                target,
                platforms,
            } = command.recipe;
            let recipe = match BuildRecipe::dockerfile(
                &schema,
                &kind,
                &context_path,
                &dockerfile_path,
                target.as_deref(),
                platforms,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalSubscription {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                connection_id: connection.id,
                installation_id: connection.installation_id.as_u64(),
                repository_identity: repository.identity(),
                branch: branch.value(),
                recipe: &recipe,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/source-subscriptions/github",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let subscription =
                GithubRepositorySubscription::subscribe(NewGithubRepositorySubscription {
                    id: SourceSubscriptionId::new(),
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                    connection_id: connection.id,
                    installation_id: connection.installation_id,
                    repository,
                    branch,
                    recipe,
                    created_at: command.created_at,
                })
                .map_err(BootError::Internal)?;
            let event =
                GithubRepositorySubscriptionCreated::envelope(&subscription, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match subscriptions
                .create(PersistGithubRepositorySubscription {
                    subscription,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateGithubRepositorySubscriptionResult {
                subscription: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalSubscription<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    connection_id: crate::modules::shared_kernel::domain::SourceConnectionId,
    installation_id: u64,
    repository_identity: &'a str,
    branch: &'a str,
    recipe: &'a BuildRecipe,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use crate::modules::sources::InMemoryGithubConnectionRepository;
    use crate::modules::sources::InMemorySourceRevisionRepository;
    use crate::modules::sources::application::resource_access::{SourceAccess, SourceAccessScope};
    use crate::modules::sources::domain::SourceRepositoryPolicy;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    struct AllowEnvironment;

    #[async_trait]
    impl ISourceEnvironmentAccess for AllowEnvironment {
        async fn require_environment(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> ApplicationResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_github_repository_subscription_fails_closed_before_creating_in_an_ungranted_environment()
     {
        let handler = CreateGithubRepositorySubscriptionHandler::from_environment_access(
            Arc::new(AllowEnvironment),
            Arc::new(InMemoryGithubConnectionRepository::new()),
            Arc::new(InMemorySourceRevisionRepository::new()),
            Arc::new(
                SourceRepositoryPolicy::github(&["https://github.com/a3s-lab/cloud".into()], &[])
                    .expect("policy"),
            ),
        );
        let result = handler
            .execute(
                CreateGithubRepositorySubscription {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                    access: SourceAccess::restricted([SourceAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
                    repository_provider: "github".into(),
                    repository_url: "https://github.com/a3s-lab/cloud".into(),
                    branch: "main".into(),
                    recipe: DockerfileBuildRecipeInput {
                        schema: "a3s.dev/source-build/v1".into(),
                        kind: "dockerfile".into(),
                        context_path: ".".into(),
                        dockerfile_path: "Dockerfile".into(),
                        target: None,
                        platforms: vec!["linux/amd64".into()],
                    },
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                    created_at: Utc::now(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "source subscriptions not found"
        ));
    }
}
