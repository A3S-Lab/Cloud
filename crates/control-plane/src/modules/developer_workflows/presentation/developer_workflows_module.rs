use super::controller::{build_plan_commands_controller, build_plan_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct DeveloperWorkflowsModule;

impl Module for DeveloperWorkflowsModule {
    fn name(&self) -> &'static str {
        "developer-workflows"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            build_plan_commands_controller(module_ref.get::<CommandBus>()?)?,
            build_plan_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
