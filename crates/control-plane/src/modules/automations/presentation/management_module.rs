use super::management_controller::{
    automation_management_queries_controller, automation_webhook_lifecycle_commands_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

/// Management presentation for authorized Automation webhook lifecycle and
/// definition catalog reads. This module does not register the public Gateway
/// receive adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationsManagementModule;

impl Module for AutomationsManagementModule {
    fn name(&self) -> &'static str {
        "automations-management"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            automation_webhook_lifecycle_commands_controller(module_ref.get::<CommandBus>()?)?,
            automation_management_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
