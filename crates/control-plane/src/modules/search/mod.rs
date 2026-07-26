pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{SearchResources, SearchResourcesHandler};
pub use domain::{ISearchRepository, SearchQuery, SearchResourceKind, SearchResult};
pub use infrastructure::{InMemorySearchRepository, PostgresSearchRepository};
pub use presentation::SearchModule;
