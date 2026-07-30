use super::controllers::{execution_commands_controller, execution_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionsModule;

impl Module for ExecutionsModule {
    fn name(&self) -> &'static str {
        "executions"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            execution_commands_controller(module_ref.get::<CommandBus>()?)?,
            execution_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
