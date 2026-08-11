mod controllers;
mod dto;
mod plugins_module;

pub use dto::{PluginCatalogInspectRequest, PluginCatalogSearchRequest, PluginRegistryResponse};
pub use plugins_module::PluginsModule;
