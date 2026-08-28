use super::{user_file_commands_controller, user_file_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct FilesModule;

impl Module for FilesModule {
    fn name(&self) -> &'static str {
        "files"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            user_file_commands_controller(module_ref.get::<CommandBus>()?)?,
            user_file_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
