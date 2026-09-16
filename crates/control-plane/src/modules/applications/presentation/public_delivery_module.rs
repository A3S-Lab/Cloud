use super::anonymous_delivery_controller::anonymous_application_delivery_commands_controller;
use super::anonymous_observation_delivery_controller::anonymous_application_observation_queries_controller;
use super::delivery_process_drain::DeliveryProcessDrain;
use a3s_boot::{
    BoxFuture, CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result,
};
use std::sync::Arc;

/// Public published-application delivery routes only (APP0.3-C3/C8).
///
/// No management create/publish/list controllers. Used by the Delivery process
/// role and composed into the management Applications module for All/Api.
#[derive(Debug, Clone, Default)]
pub struct ApplicationPublicDeliveryModule {
    drain: DeliveryProcessDrain,
}

impl ApplicationPublicDeliveryModule {
    pub fn new(drain: DeliveryProcessDrain) -> Self {
        Self { drain }
    }

    pub(crate) fn public_controllers(
        command_bus: Arc<CommandBus>,
        query_bus: Arc<QueryBus>,
        drain: DeliveryProcessDrain,
    ) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            anonymous_application_delivery_commands_controller(command_bus, drain)?,
            anonymous_application_observation_queries_controller(query_bus)?,
        ])
    }
}

impl Module for ApplicationPublicDeliveryModule {
    fn name(&self) -> &'static str {
        "application-public-delivery"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Self::public_controllers(
            module_ref.get::<CommandBus>()?,
            module_ref.get::<QueryBus>()?,
            self.drain.clone(),
        )
    }

    fn before_application_shutdown(
        &self,
        _module_ref: ModuleRef,
        _signal: Option<String>,
    ) -> BoxFuture<'static, Result<()>> {
        let drain = self.drain.clone();
        Box::pin(async move {
            drain.begin();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_delivery_module_exposes_only_anonymous_delivery_prefixes() {
        let controllers = ApplicationPublicDeliveryModule::public_controllers(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
            DeliveryProcessDrain::default(),
        )
        .expect("public delivery controllers");
        assert_eq!(controllers.len(), 2);
        for controller in &controllers {
            assert_eq!(
                controller.prefix(),
                "/anonymous-delivery",
                "delivery module leaked non-anonymous prefix {}",
                controller.prefix()
            );
        }
        assert_eq!(controllers[0].routes().len(), 4);
        assert_eq!(controllers[1].routes().len(), 3);
    }
}
