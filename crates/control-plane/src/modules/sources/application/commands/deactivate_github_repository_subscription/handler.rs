use super::{DeactivateGithubRepositorySubscription, DeactivateGithubRepositorySubscriptionResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use crate::modules::sources::application::ISourceEnvironmentAccess;
use crate::modules::sources::domain::{
    DeactivateGithubRepositorySubscription as PersistDeactivation,
    GithubRepositorySubscriptionDeactivated, ISourceSubscriptionRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct DeactivateGithubRepositorySubscriptionHandler {
    environment_access: Arc<dyn ISourceEnvironmentAccess>,
    subscriptions: Arc<dyn ISourceSubscriptionRepository>,
}

impl DeactivateGithubRepositorySubscriptionHandler {
    pub(in crate::modules::sources) fn from_environment_access(
        environment_access: Arc<dyn ISourceEnvironmentAccess>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    ) -> Self {
        Self {
            environment_access,
            subscriptions,
        }
    }
}

impl CommandHandler<DeactivateGithubRepositorySubscription>
    for DeactivateGithubRepositorySubscriptionHandler
{
    fn execute(
        &self,
        command: DeactivateGithubRepositorySubscription,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DeactivateGithubRepositorySubscriptionResult>>,
    > {
        let environment_access = Arc::clone(&self.environment_access);
        let subscriptions = Arc::clone(&self.subscriptions);
        Box::pin(async move {
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
            let mut subscription = match subscriptions
                .find(command.organization_id, command.subscription_id)
                .await
            {
                Ok(Some(value))
                    if value.project_id == command.project_id
                        && value.environment_id == command.environment_id =>
                {
                    value
                }
                Ok(_) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "GitHub repository subscription not found in environment".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "subscriptionId": command.subscription_id,
                "action": "deactivate",
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/source-subscriptions/github/{}/deactivate",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.subscription_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let previous_version = subscription.aggregate_version;
            subscription
                .deactivate(command.deactivated_at)
                .map_err(BootError::Internal)?;
            let event = GithubRepositorySubscriptionDeactivated::envelope(
                &subscription,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match subscriptions
                .deactivate(PersistDeactivation {
                    subscription,
                    previous_version,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(DeactivateGithubRepositorySubscriptionResult {
                subscription: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
