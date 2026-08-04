use super::controllers::{agent_commands_controller, agent_queries_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentsModule;

impl Module for AgentsModule {
    fn name(&self) -> &'static str {
        "agents"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            agent_commands_controller(module_ref.get::<CommandBus>()?)?,
            agent_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
