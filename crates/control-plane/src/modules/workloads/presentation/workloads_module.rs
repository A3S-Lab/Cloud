use super::controllers::{workload_queries_controller, workloads_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkloadsModule;

impl Module for WorkloadsModule {
    fn name(&self) -> &'static str {
        "workloads"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            workloads_controller(module_ref.get::<CommandBus>()?)?,
            workload_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
