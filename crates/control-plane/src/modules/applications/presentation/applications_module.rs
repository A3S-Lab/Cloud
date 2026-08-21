use super::controller::{application_commands_controller, application_queries_controller};
use super::delivery_controller::{
    application_delivery_commands_controller, application_delivery_queries_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplicationsModule;

impl Module for ApplicationsModule {
    fn name(&self) -> &'static str {
        "applications"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            application_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_delivery_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_delivery_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
