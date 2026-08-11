pub mod domain;
pub mod infrastructure;

pub use infrastructure::persistence::{
    InMemoryPluginRegistryRepository, PostgresPluginRegistryRepository,
};
