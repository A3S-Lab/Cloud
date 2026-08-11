pub mod commands;
pub mod queries;

pub use commands::{EnrollPluginRegistry, EnrollPluginRegistryHandler, EnrollPluginRegistryResult};
pub use queries::{
    GetPluginRegistry, GetPluginRegistryHandler, ListPluginRegistries, ListPluginRegistriesHandler,
};

#[cfg(test)]
mod tests;
