use super::controller::{notification_commands_controller, notification_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct NotificationsModule;

impl Module for NotificationsModule {
    fn name(&self) -> &'static str {
        "notifications"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            notification_queries_controller(module_ref.get::<QueryBus>()?)?,
            notification_commands_controller(module_ref.get::<CommandBus>()?)?,
        ])
    }
}
