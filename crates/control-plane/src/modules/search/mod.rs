mod application;
mod domain;
mod infrastructure;
mod presentation;

use a3s_orm::PostgresExecutor;
use infrastructure::PostgresSearchRepository;
use std::sync::Arc;

pub use application::{SearchResources, SearchResourcesHandler};
pub use domain::{
    ISearchRepository, SearchQuery, SearchResourceKind, SearchResult, SearchVisibility,
    SearchVisibilityScope,
};
pub(crate) use presentation::{SearchModule, SearchResultResponse};

/// Builds the production persistence adapter inside the Search owner boundary
/// and exposes only the domain port to process composition and conformance.
pub(crate) fn search_persistence_adapter(executor: PostgresExecutor) -> Arc<dyn ISearchRepository> {
    Arc::new(PostgresSearchRepository::new(executor))
}

#[cfg(test)]
pub(crate) use infrastructure::InMemorySearchRepository;
