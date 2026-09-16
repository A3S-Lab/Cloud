use super::authenticated_delivery_controller::authenticated_application_delivery_commands_controller;
use super::authenticated_observation_delivery_controller::authenticated_application_observation_queries_controller;
use super::delivery_process_drain::DeliveryProcessDrain;
use a3s_boot::{
    BoxFuture, CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result,
};
use std::sync::Arc;

/// Authenticated published-application delivery routes (APP0.3-C5/C6/C7/C8).
///
/// Mounted on ProcessRole::Delivery with closed Identity read verification, and
/// composed into ApplicationsModule so management OpenAPI documents `/delivery`
/// without registering management create/publish/list controllers on Delivery.
#[derive(Debug, Clone, Default)]
pub struct ApplicationAuthenticatedDeliveryModule {
    drain: DeliveryProcessDrain,
}

impl ApplicationAuthenticatedDeliveryModule {
    pub fn new(drain: DeliveryProcessDrain) -> Self {
        Self { drain }
    }

    pub(crate) fn authenticated_controllers(
        command_bus: Arc<CommandBus>,
        query_bus: Arc<QueryBus>,
        drain: DeliveryProcessDrain,
    ) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            authenticated_application_delivery_commands_controller(command_bus, drain)?,
            authenticated_application_observation_queries_controller(query_bus)?,
        ])
    }
}

impl Module for ApplicationAuthenticatedDeliveryModule {
    fn name(&self) -> &'static str {
        "application-authenticated-delivery"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Self::authenticated_controllers(
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
    fn authenticated_delivery_module_exposes_only_delivery_prefix() {
        let controllers = ApplicationAuthenticatedDeliveryModule::authenticated_controllers(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
            DeliveryProcessDrain::default(),
        )
        .expect("authenticated delivery controllers");
        assert_eq!(controllers.len(), 2);
        assert!(
            controllers
                .iter()
                .all(|controller| controller.prefix() == "/delivery")
        );
        let route_count: usize = controllers.iter().map(|c| c.routes().len()).sum();
        assert_eq!(route_count, 7);
    }
}
