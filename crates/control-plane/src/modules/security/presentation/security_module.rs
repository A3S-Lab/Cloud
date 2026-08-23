use super::controller::security_investigation_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct SecurityModule;

impl Module for SecurityModule {
    fn name(&self) -> &'static str {
        "security"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![security_investigation_controller(
            module_ref.get::<QueryBus>()?,
        )?])
    }
}
