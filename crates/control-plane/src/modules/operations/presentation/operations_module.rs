use super::controllers::operations_query_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct OperationsModule;

impl Module for OperationsModule {
    fn name(&self) -> &'static str {
        "operations"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![operations_query_controller(
            module_ref.get::<QueryBus>()?,
        )?])
    }
}
