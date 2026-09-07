//! AUT0.2 is component-only at this stage.
//!
//! The controller adapter is available to an owner-composed listener, but no
//! `Module` registration is exported here. Gateway and management presentation
//! remain gated on signature verification, registry selection, durable
//! recovery, and retained integration evidence.

mod webhook_controller;
mod webhook_transport;

pub use webhook_controller::automation_webhooks_controller;
pub use webhook_transport::AutomationWebhookTransportRequest;
