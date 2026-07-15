use super::controllers::{
    enrollment_controller, node_management_controller, node_queries_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use chrono::Duration;

#[derive(Debug, Clone, Copy)]
pub struct FleetModule {
    heartbeat_timeout: Duration,
}

impl FleetModule {
    pub fn new(heartbeat_timeout: Duration) -> Result<Self> {
        if heartbeat_timeout <= Duration::zero() {
            return Err(a3s_boot::BootError::Internal(
                "node heartbeat timeout must be positive".into(),
            ));
        }
        Ok(Self { heartbeat_timeout })
    }
}

impl Module for FleetModule {
    fn name(&self) -> &'static str {
        "fleet"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let commands = module_ref.get::<CommandBus>()?;
        Ok(vec![
            enrollment_controller(commands.clone())?,
            node_management_controller(commands, self.heartbeat_timeout)?,
            node_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
