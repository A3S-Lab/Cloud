use super::webhook_controller::automation_webhooks_controller;
use crate::modules::automations::application::{
    AutomationWebhookReceiver, ReceiveAutomationWebhookDelivery,
    ReceiveAutomationWebhookDeliveryHandler,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, Result};
use std::fmt;
use std::sync::Arc;

/// Owner-composed Automations presentation module.
///
/// The caller must provide a fully composed receiver. This module never
/// creates an authorization, Secret, schema-registry, or persistence adapter;
/// it only registers the receive command and its scoped transport adapter.
#[derive(Clone)]
pub struct AutomationsModule {
    receiver: Arc<AutomationWebhookReceiver>,
}

impl AutomationsModule {
    pub fn new(receiver: Arc<AutomationWebhookReceiver>) -> Self {
        Self { receiver }
    }
}

impl fmt::Debug for AutomationsModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AutomationsModule")
            .finish_non_exhaustive()
    }
}

impl Module for AutomationsModule {
    fn name(&self) -> &'static str {
        "automations"
    }

    fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        module_ref
            .get::<CommandBus>()?
            .register::<ReceiveAutomationWebhookDelivery, _>(
                ReceiveAutomationWebhookDeliveryHandler::new(Arc::clone(&self.receiver)),
            )
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![automation_webhooks_controller(
            module_ref.get::<CommandBus>()?,
        )?])
    }
}
