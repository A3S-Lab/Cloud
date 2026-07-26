use super::GetApiToken;
use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetApiTokenHandler {
    repository: Arc<dyn IApiTokenRepository>,
}

impl GetApiTokenHandler {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetApiToken> for GetApiTokenHandler {
    fn execute(
        &self,
        query: GetApiToken,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApiToken>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository.find(query.organization_id, query.token_id).await {
                Ok(Some(token)) => Ok(Ok(token)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "API token not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
