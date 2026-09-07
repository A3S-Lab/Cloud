use super::controllers::{
    plugin_assignment_commands_controller, plugin_assignment_queries_controller,
    plugin_plan_projection_commands_controller, plugin_plan_projection_queries_controller,
    plugin_registry_queries_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct PluginsModule;

impl Module for PluginsModule {
    fn name(&self) -> &'static str {
        "plugins"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            plugin_registry_queries_controller(module_ref.get::<QueryBus>()?)?,
            plugin_assignment_commands_controller(module_ref.get::<CommandBus>()?)?,
            plugin_assignment_queries_controller(module_ref.get::<QueryBus>()?)?,
            plugin_plan_projection_commands_controller(module_ref.get::<CommandBus>()?)?,
            plugin_plan_projection_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
