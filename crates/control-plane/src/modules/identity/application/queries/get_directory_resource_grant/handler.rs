use super::GetDirectoryResourceGrant;
use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::domain::repositories::IDirectoryResourceGrantRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetDirectoryResourceGrantHandler {
    repository: Arc<dyn IDirectoryResourceGrantRepository>,
}

impl GetDirectoryResourceGrantHandler {
    pub fn new(repository: Arc<dyn IDirectoryResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetDirectoryResourceGrant> for GetDirectoryResourceGrantHandler {
    fn execute(
        &self,
        query: GetDirectoryResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<DirectoryResourceGrant>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_directory_resource_grant(query.organization_id, query.resource_grant_id)
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Directory Resource Grant not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
