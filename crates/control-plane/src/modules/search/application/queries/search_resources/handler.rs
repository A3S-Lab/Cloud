use super::SearchResources;
use crate::modules::search::domain::{ISearchRepository, SearchQuery, SearchResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub const MAXIMUM_SEARCH_RESULTS: u16 = 50;

pub struct SearchResourcesHandler {
    repository: Arc<dyn ISearchRepository>,
}

impl SearchResourcesHandler {
    pub fn new(repository: Arc<dyn ISearchRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<SearchResources> for SearchResourcesHandler {
    fn execute(
        &self,
        query: SearchResources,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<SearchResult>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let normalized = match SearchQuery::parse(query.query) {
                Ok(query) => query,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if query.limit == 0 || query.limit > MAXIMUM_SEARCH_RESULTS {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "search result limit must be between 1 and {MAXIMUM_SEARCH_RESULTS}"
                ))));
            }
            Ok(repository
                .search(query.organization_id, &normalized, query.limit)
                .await
                .map_err(Into::into))
        })
    }
}
