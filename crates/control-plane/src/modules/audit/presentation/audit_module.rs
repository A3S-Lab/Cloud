use super::controller::audit_query_controller;
use a3s_boot::{ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AuditModule;

impl Module for AuditModule {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![audit_query_controller(module_ref.get::<QueryBus>()?)?])
    }
}
