use super::{CreateGithubRepositorySubscription, CreateGithubRepositorySubscriptionResult};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, SourceSubscriptionId};
use crate::modules::sources::application::commands::resolve_external_source_revision::DockerfileBuildRecipeInput;
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
    environments: Arc<dyn IEnvironmentRepository>,
    connections: Arc<dyn IGithubConnectionRepository>,
    subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    policy: Arc<SourceRepositoryPolicy>,
}

impl CreateGithubRepositorySubscriptionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        connections: Arc<dyn IGithubConnectionRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self {
            environments,
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
        let environments = Arc::clone(&self.environments);
        let connections = Arc::clone(&self.connections);
        let subscriptions = Arc::clone(&self.subscriptions);
        let policy = Arc::clone(&self.policy);
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let connection = match connections.find(command.organization_id).await {
                Ok(Some(value)) if value.is_authoritative() => value,
                Ok(Some(_)) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "GitHub source connection is not active".into(),
                    )))
                }
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "verified GitHub source connection not found for organization".into(),
                    )))
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
