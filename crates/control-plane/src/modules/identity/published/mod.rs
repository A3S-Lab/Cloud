//! Immutable facts published by the Identity bounded context.
//!
//! Consumers receive only admitted owner lineage and provider-neutral Runtime
//! semantics. Identity policy aggregates, trust-domain lifecycle, credentials,
//! private keys, and management authorization remain private to Identity.

mod workload_runtime_execution_authorization;

pub(in crate::modules::identity) use workload_runtime_execution_authorization::ValidatedWorkloadRuntimeExecutionAuthorizationProjection;
pub use workload_runtime_execution_authorization::{
    WorkloadRuntimeExecutionAuthorization, WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA,
};
