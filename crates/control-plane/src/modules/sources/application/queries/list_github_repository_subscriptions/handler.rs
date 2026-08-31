use super::ListGithubRepositorySubscriptions;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::application::ISourceEnvironmentAccess;
use crate::modules::sources::domain::{
    GithubRepositorySubscription, ISourceSubscriptionRepository,
};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListGithubRepositorySubscriptionsHandler {
    environment_access: Arc<dyn ISourceEnvironmentAccess>,
    subscriptions: Arc<dyn ISourceSubscriptionRepository>,
}

impl ListGithubRepositorySubscriptionsHandler {
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

impl QueryHandler<ListGithubRepositorySubscriptions> for ListGithubRepositorySubscriptionsHandler {
    fn execute(
        &self,
        query: ListGithubRepositorySubscriptions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<GithubRepositorySubscription>>>,
    > {
        let environment_access = Arc::clone(&self.environment_access);
        let subscriptions = Arc::clone(&self.subscriptions);
        Box::pin(async move {
            if let Err(error) = environment_access
                .require_environment(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                return Ok(Err(error));
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
