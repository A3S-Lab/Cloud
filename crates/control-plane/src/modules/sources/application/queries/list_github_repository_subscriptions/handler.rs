use super::ListGithubRepositorySubscriptions;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::sources::domain::{
    GithubRepositorySubscription, ISourceSubscriptionRepository,
};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListGithubRepositorySubscriptionsHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    subscriptions: Arc<dyn ISourceSubscriptionRepository>,
}

impl ListGithubRepositorySubscriptionsHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    ) -> Self {
        Self {
            environments,
            subscriptions,
        }
    }
}

impl QueryHandler<ListGithubRepositorySubscriptions> for ListGithubRepositorySubscriptionsHandler {
    fn execute(
        &self,
        query: ListGithubRepositorySubscriptions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<GithubRepositorySubscription>>>,
    > {
        let environments = Arc::clone(&self.environments);
        let subscriptions = Arc::clone(&self.subscriptions);
        Box::pin(async move {
            match environments
                .find(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
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
            match subscriptions
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
