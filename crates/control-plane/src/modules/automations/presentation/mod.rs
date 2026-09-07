//! AUT0.2 is component-only at this stage.
//!
//! No controller or `Module` registration is exported here. The framework-
//! neutral transport parser is available to a future listener, while Gateway
//! and management presentation remain gated on signature verification,
//! registry selection, durable recovery, and retained integration evidence.

mod webhook_transport;

pub use webhook_transport::AutomationWebhookTransportRequest;
