//! Immutable facts published by the Workloads bounded context.
//!
//! Consumers receive only exact binding evidence and the provider-neutral
//! Runtime contract. ResourceClaim, Deployment, replica, and revision
//! lifecycles remain private to Workloads.

mod bound_runtime_claim;

pub(in crate::modules::workloads) use bound_runtime_claim::ValidatedBoundRuntimeClaimProjection;
pub use bound_runtime_claim::{BoundRuntimeClaim, BOUND_RUNTIME_CLAIM_SCHEMA};
