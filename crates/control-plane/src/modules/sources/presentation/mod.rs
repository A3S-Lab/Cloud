mod controllers;
mod dto;
mod routes;
mod sources_module;

pub(crate) use dto::{
    GithubRepositoryDiscoveryPageResponse, GithubRepositoryReferenceDiscoveryPageResponse,
};
pub(crate) use routes::{
    GITHUB_REPOSITORY_DISCOVERY_ROUTE, GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
    GITHUB_SOURCE_CONNECTION_ROUTE, SOURCES_CONTROLLER_PREFIX,
};
pub use sources_module::SourcesModule;
