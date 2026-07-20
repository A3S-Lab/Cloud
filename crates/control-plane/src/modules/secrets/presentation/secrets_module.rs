use super::controllers::{secret_queries_controller, secrets_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct SecretsModule;

impl Module for SecretsModule {
    fn name(&self) -> &'static str {
        "secrets"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            secrets_controller(module_ref.get::<CommandBus>()?)?,
            secret_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
