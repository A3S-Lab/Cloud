use super::controllers::{form_commands_controller, form_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct FormsModule;

impl Module for FormsModule {
    fn name(&self) -> &'static str {
        "forms"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            form_commands_controller(module_ref.get::<CommandBus>()?)?,
            form_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
