use super::controllers::{
    domain_claim_commands_controller, domain_claim_queries_controller, route_queries_controller,
    routes_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeModule;

impl Module for EdgeModule {
    fn name(&self) -> &'static str {
        "edge"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            domain_claim_commands_controller(module_ref.get::<CommandBus>()?)?,
            domain_claim_queries_controller(module_ref.get::<QueryBus>()?)?,
            routes_controller(module_ref.get::<CommandBus>()?)?,
            route_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
