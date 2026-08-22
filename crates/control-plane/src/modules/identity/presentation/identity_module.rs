use super::controllers::{
    api_token_controller, bootstrap_controller, membership_controller,
    membership_invitation_acceptance_controller, membership_invitation_administration_controller,
    membership_invitation_self_query_controller, oidc_link_controller, oidc_public_controller,
    organization_controller, organizations_query_controller, recipient_contact_commands_controller,
    recipient_contact_queries_controller, resource_grant_controller,
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
        let command_bus = module_ref.get::<CommandBus>()?;
        Ok(vec![
            bootstrap_controller(
                command_bus.clone(),
                BootstrapGuard::new(self.bootstrap_credential.clone()),
            )?,
            organization_controller(command_bus.clone())?,
            api_token_controller(command_bus.clone(), module_ref.get::<QueryBus>()?)?,
            membership_controller(command_bus.clone(), module_ref.get::<QueryBus>()?)?,
            membership_invitation_administration_controller(
                command_bus.clone(),
                module_ref.get::<QueryBus>()?,
            )?,
            membership_invitation_self_query_controller(module_ref.get::<QueryBus>()?)?,
            membership_invitation_acceptance_controller(command_bus.clone())?,
            resource_grant_controller(command_bus, module_ref.get::<QueryBus>()?)?,
            organizations_query_controller(module_ref.get::<QueryBus>()?)?,
            recipient_contact_queries_controller(module_ref.get::<QueryBus>()?)?,
            recipient_contact_commands_controller(module_ref.get::<CommandBus>()?)?,
            oidc_public_controller(module_ref.get::<CommandBus>()?)?,
            oidc_link_controller(module_ref.get::<CommandBus>()?)?,
        ])
    }
}
