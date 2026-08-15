use super::controller::{connector_commands_controller, connector_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectorsModule;

impl Module for ConnectorsModule {
    fn name(&self) -> &'static str {
        "connectors"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            connector_commands_controller(module_ref.get::<CommandBus>()?)?,
            connector_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
