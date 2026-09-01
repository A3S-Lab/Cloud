//! Immutable facts published by the Workloads bounded context.
//!
//! Consumers receive only exact binding evidence and the provider-neutral
//! Runtime contract. ResourceClaim, Deployment, replica, and revision
//! lifecycles remain private to Workloads.

mod authorized_secret_materialization;
mod bound_runtime_claim;

pub(in crate::modules::workloads) use authorized_secret_materialization::ValidatedSecretMaterializationProjection;
pub use authorized_secret_materialization::{
    AuthorizedWorkloadSecretMaterialization, AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA,
};
pub(in crate::modules::workloads) use bound_runtime_claim::ValidatedBoundRuntimeClaimProjection;
pub use bound_runtime_claim::{BoundRuntimeClaim, BOUND_RUNTIME_CLAIM_SCHEMA};
