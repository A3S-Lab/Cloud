use super::ListDirectoryResourceGrants;
use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::domain::repositories::IDirectoryResourceGrantRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListDirectoryResourceGrantsHandler {
    repository: Arc<dyn IDirectoryResourceGrantRepository>,
}

impl ListDirectoryResourceGrantsHandler {
    pub fn new(repository: Arc<dyn IDirectoryResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListDirectoryResourceGrants> for ListDirectoryResourceGrantsHandler {
    fn execute(
        &self,
        query: ListDirectoryResourceGrants,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<DirectoryResourceGrant>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .list_directory_resource_grants_by_organization(query.organization_id)
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
