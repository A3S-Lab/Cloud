use super::{
    inference_route_commands_controller, inference_route_queries_controller,
    usage_queries_controller, usage_retention_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct InferenceModule;

impl Module for InferenceModule {
    fn name(&self) -> &'static str {
        "inference"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let command_bus = module_ref.get::<CommandBus>()?;
        let query_bus = module_ref.get::<QueryBus>()?;
        Ok(vec![
            inference_route_commands_controller(Arc::clone(&command_bus))?,
            inference_route_queries_controller(Arc::clone(&query_bus))?,
            usage_queries_controller(Arc::clone(&query_bus))?,
            usage_retention_controller(query_bus)?,
        ])
    }
}
