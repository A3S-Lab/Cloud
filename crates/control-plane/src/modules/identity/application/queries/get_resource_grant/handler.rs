use super::GetResourceGrant;
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::domain::repositories::IResourceGrantRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetResourceGrantHandler {
    repository: Arc<dyn IResourceGrantRepository>,
}

impl GetResourceGrantHandler {
    pub fn new(repository: Arc<dyn IResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetResourceGrant> for GetResourceGrantHandler {
    fn execute(
        &self,
        query: GetResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ResourceGrant>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_resource_grant(query.organization_id, query.resource_grant_id)
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Resource Grant not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
