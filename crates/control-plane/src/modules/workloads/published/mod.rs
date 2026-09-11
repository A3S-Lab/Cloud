//! Immutable facts published by the Workloads bounded context.
//!
//! Consumers receive only exact binding evidence and the provider-neutral
//! Runtime contract. ResourceClaim, Deployment, replica, and revision
//! lifecycles remain private to Workloads.

mod active_mcp_revision_projection;
mod authorized_secret_materialization;
mod bound_runtime_claim;
mod healthy_route_target_candidates;

pub(in crate::modules::workloads) use active_mcp_revision_projection::ValidatedActiveMcpWorkloadRevisionProjection;
pub use active_mcp_revision_projection::{
    ActiveMcpWorkloadRevisionProjection, ACTIVE_MCP_WORKLOAD_REVISION_PROJECTION_SCHEMA,
};
pub(in crate::modules::workloads) use authorized_secret_materialization::ValidatedSecretMaterializationProjection;
pub use authorized_secret_materialization::{
    AuthorizedWorkloadSecretMaterialization, AUTHORIZED_WORKLOAD_SECRET_MATERIALIZATION_SCHEMA,
};
pub(in crate::modules::workloads) use bound_runtime_claim::ValidatedBoundRuntimeClaimProjection;
pub use bound_runtime_claim::{BoundRuntimeClaim, BOUND_RUNTIME_CLAIM_SCHEMA};
pub(in crate::modules::workloads) use healthy_route_target_candidates::{
    ValidatedWorkloadHealthyRouteTargetCandidate, ValidatedWorkloadHealthyRouteTargetCandidateSet,
};
pub use healthy_route_target_candidates::{
    WorkloadHealthyRouteTargetCandidate, WorkloadHealthyRouteTargetCandidateSet,
    WORKLOAD_HEALTHY_ROUTE_TARGET_CANDIDATE_SET_SCHEMA,
};
