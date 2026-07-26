use super::controllers::search_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchModule;

impl Module for SearchModule {
    fn name(&self) -> &'static str {
        "search"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![search_controller(module_ref.get::<QueryBus>()?)?])
    }
}
