use super::{usage_queries_controller, usage_retention_controller};
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct InferenceModule;

impl Module for InferenceModule {
    fn name(&self) -> &'static str {
        "inference"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let bus = module_ref.get::<QueryBus>()?;
        Ok(vec![
            usage_queries_controller(Arc::clone(&bus))?,
            usage_retention_controller(bus)?,
        ])
    }
}
