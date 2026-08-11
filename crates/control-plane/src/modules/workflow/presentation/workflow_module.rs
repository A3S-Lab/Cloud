use super::controllers::{
    ontology_commands_controller, ontology_queries_controller, workflow_commands_controller,
    workflow_queries_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowModule;

impl Module for WorkflowModule {
    fn name(&self) -> &'static str {
        "workflow"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            ontology_commands_controller(module_ref.get::<CommandBus>()?)?,
            ontology_queries_controller(module_ref.get::<QueryBus>()?)?,
            workflow_commands_controller(module_ref.get::<CommandBus>()?)?,
            workflow_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
