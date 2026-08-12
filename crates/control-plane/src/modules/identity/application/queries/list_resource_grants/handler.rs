use super::ListResourceGrants;
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::domain::repositories::IResourceGrantRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListResourceGrantsHandler {
    repository: Arc<dyn IResourceGrantRepository>,
}

impl ListResourceGrantsHandler {
    pub fn new(repository: Arc<dyn IResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListResourceGrants> for ListResourceGrantsHandler {
    fn execute(
        &self,
        query: ListResourceGrants,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ResourceGrant>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .list_resource_grants(query.organization_id, query.membership_id)
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
