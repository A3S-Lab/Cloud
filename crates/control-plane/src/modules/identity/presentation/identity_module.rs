use super::controllers::{
    api_token_controller, bootstrap_controller, organization_controller,
    organizations_query_controller,
};
use super::BootstrapGuard;
use crate::modules::identity::domain::value_objects::BootstrapCredential;
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Clone)]
pub struct IdentityModule {
    bootstrap_credential: BootstrapCredential,
}

impl IdentityModule {
    pub fn new(bootstrap_credential: BootstrapCredential) -> Self {
        Self {
            bootstrap_credential,
        }
    }
}

impl Module for IdentityModule {
    fn name(&self) -> &'static str {
        "identity"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let bus = module_ref.get::<CommandBus>()?;
        Ok(vec![
            bootstrap_controller(
                bus.clone(),
                BootstrapGuard::new(self.bootstrap_credential.clone()),
            )?,
            organization_controller(bus.clone())?,
            api_token_controller(bus)?,
            organizations_query_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
