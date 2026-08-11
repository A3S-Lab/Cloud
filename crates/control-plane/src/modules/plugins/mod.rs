pub mod application;
pub mod domain;
pub mod infrastructure;

#[cfg(test)]
pub(crate) mod test_support;

pub use application::{
    EnrollPluginRegistry, EnrollPluginRegistryHandler, EnrollPluginRegistryResult,
    GetPluginRegistry, GetPluginRegistryHandler, ListPluginRegistries, ListPluginRegistriesHandler,
};

pub use infrastructure::{
    persistence::{InMemoryPluginRegistryRepository, PostgresPluginRegistryRepository},
    A3sUsePluginRegistryCatalog, PluginTrustRootObjectStore,
};
