use super::controller::{
    durable_cell_commands_controller, durable_cell_queries_controller,
    durable_cell_route_commands_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct DurableCellsModule;

impl Module for DurableCellsModule {
    fn name(&self) -> &'static str {
        "durable-cells"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let commands = module_ref.get::<CommandBus>()?;
        Ok(vec![
            durable_cell_commands_controller(commands.clone())?,
            durable_cell_route_commands_controller(commands)?,
            durable_cell_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
