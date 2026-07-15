use super::controllers::{route_queries_controller, routes_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeModule;

impl Module for EdgeModule {
    fn name(&self) -> &'static str {
        "edge"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            routes_controller(module_ref.get::<CommandBus>()?)?,
            route_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
