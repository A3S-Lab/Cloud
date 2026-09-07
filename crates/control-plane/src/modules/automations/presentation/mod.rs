//! AUT0.2 is component-only at this stage.
//!
//! The controller adapter and owner-composed `AutomationsModule` are exported,
//! but the current Cloud application does not construct or import that module
//! until its production receiver dependencies are available. Gateway and
//! management presentation remain gated on signature verification, registry
//! selection, durable recovery, and retained integration evidence.

mod automations_module;
mod webhook_controller;
mod webhook_transport;

pub use automations_module::AutomationsModule;
pub use webhook_controller::automation_webhooks_controller;
pub use webhook_transport::AutomationWebhookTransportRequest;
