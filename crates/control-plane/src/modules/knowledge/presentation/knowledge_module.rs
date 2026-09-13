use super::{knowledge_commands_controller, knowledge_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct KnowledgeModule;

impl Module for KnowledgeModule {
    fn name(&self) -> &'static str {
        "knowledge"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            knowledge_commands_controller(module_ref.get::<CommandBus>()?)?,
            knowledge_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
