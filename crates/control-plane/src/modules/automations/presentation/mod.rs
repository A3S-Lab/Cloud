//! Automations presentation adapters.
//!
//! Management lifecycle/catalog controllers are registered through
//! `AutomationsManagementModule`. The public webhook receive adapter remains
//! gated on signature verification, registry selection, and owner composition.

mod automations_module;
mod dto;
mod management_controller;
mod management_module;
mod webhook_controller;
mod webhook_transport;

pub use automations_module::AutomationsModule;
pub use management_module::AutomationsManagementModule;
pub use webhook_controller::automation_webhooks_controller;
pub use webhook_transport::AutomationWebhookTransportRequest;
pub use dto::{
    AutomationDefinitionResponse, AutomationRevisionResponse, AutomationWebhookEndpointResponse,
};
