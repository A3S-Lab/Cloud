mod controllers;
mod dto;
mod plugins_module;

pub use dto::{
    ConfirmPluginPlanProjectionRequest, EnrollPluginRegistryRequest,
    PluginAssignmentMutationResponse, PluginAssignmentResponse, PluginCatalogInspectRequest,
    PluginCatalogSearchRequest, PluginPlanProjectionResponse, PluginRegistryMutationResponse,
    PluginRegistryResponse, SetPluginAssignmentRequest,
};
pub use plugins_module::PluginsModule;
