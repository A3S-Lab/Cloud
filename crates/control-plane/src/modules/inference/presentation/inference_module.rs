use super::usage_queries_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct InferenceModule;

impl Module for InferenceModule {
    fn name(&self) -> &'static str {
        "inference"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![usage_queries_controller(
            module_ref.get::<QueryBus>()?,
        )?])
    }
}
