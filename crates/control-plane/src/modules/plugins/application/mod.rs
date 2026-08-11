pub mod commands;
pub mod queries;

pub use commands::{EnrollPluginRegistry, EnrollPluginRegistryHandler, EnrollPluginRegistryResult};
pub use queries::{
    GetPluginRegistry, GetPluginRegistryHandler, InspectCachedPluginCatalog,
    InspectCachedPluginCatalogHandler, InspectPluginCatalog, InspectPluginCatalogHandler,
    ListPluginRegistries, ListPluginRegistriesHandler, SearchCachedPluginCatalog,
    SearchCachedPluginCatalogHandler, SearchPluginCatalog, SearchPluginCatalogHandler,
};

#[cfg(test)]
mod tests;
