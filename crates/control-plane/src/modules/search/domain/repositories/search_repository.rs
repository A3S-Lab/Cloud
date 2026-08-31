use crate::modules::search::domain::{SearchQuery, SearchResult, SearchVisibility};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use async_trait::async_trait;

#[async_trait]
pub trait ISearchRepository: Send + Sync {
    async fn search(
        &self,
        organization_id: OrganizationId,
        query: &SearchQuery,
        limit: u16,
        visibility: &SearchVisibility,
    ) -> Result<Vec<SearchResult>, RepositoryError>;
}
