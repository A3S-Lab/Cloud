use super::controllers::plugin_registry_queries_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct PluginsModule;

impl Module for PluginsModule {
    fn name(&self) -> &'static str {
        "plugins"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![plugin_registry_queries_controller(
            module_ref.get::<QueryBus>()?,
        )?])
    }
}
