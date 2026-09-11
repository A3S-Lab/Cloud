use super::ListGithubRepositorySubscriptions;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "source subscriptions not found".into(),
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use crate::modules::sources::InMemorySourceRevisionRepository;
    use crate::modules::sources::application::resource_access::{SourceAccess, SourceAccessScope};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;

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
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let handler = ListGithubRepositorySubscriptionsHandler::from_environment_access(
            Arc::new(AllowEnvironment),
            Arc::new(InMemorySourceRevisionRepository::new()),
        );
        let result = handler
            .execute(
                ListGithubRepositorySubscriptions {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                    access: SourceAccess::restricted([SourceAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
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
