use super::controllers::{build_run_commands_controller, build_run_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ArtifactsModule;

impl Module for ArtifactsModule {
    fn name(&self) -> &'static str {
        "artifacts"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            build_run_commands_controller(module_ref.get::<CommandBus>()?)?,
            build_run_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
