//! Deliberate public Data contracts.
//!
//! The concrete `infrastructure` module stays private. Selected recovery
//! request DTOs are re-exported from the bounded-context root so consumers
//! never depend on an outer-layer module path.

pub use super::infrastructure::ObjectNamespaceRecoveryOperationRequest;
