use super::ListApiTokens;
use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListApiTokensHandler {
    repository: Arc<dyn IApiTokenRepository>,
}

impl ListApiTokensHandler {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListApiTokens> for ListApiTokensHandler {
    fn execute(
        &self,
        query: ListApiTokens,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ApiToken>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id)
                .await
                .map_err(Into::into))
        })
    }
}
