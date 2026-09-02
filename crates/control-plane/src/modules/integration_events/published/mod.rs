//! Stable Published Language emitted by the Integration Events bounded context.
//!
//! Provider metadata carries event identity, type, and schema. The payload below carries the
//! canonical committed Cloud scope and aggregate fact consumed by external adapters.

mod outbox_envelope;

pub use outbox_envelope::PublishedOutboxEnvelope;
