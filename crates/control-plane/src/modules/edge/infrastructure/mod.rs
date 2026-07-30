mod deployment_route_updater;
mod domain_ownership_verifier;
mod gateway_acknowledgement_projector;
mod gateway_certificate_reconciler;
mod gateway_command_queue;
mod gateway_observation_queue;
mod gateway_replica_recovery_reconciler;
mod gateway_rollout_reconciler;
mod gateway_rollout_rollback_compiler;
mod gateway_rollout_rollback_reconciler;
mod gateway_route_rollout_compiler;
mod gateway_route_rollout_planner;
mod gateway_snapshot_compiler;
mod local_gateway_certificate_authority;
mod mcp_credential_issuer;
mod mcp_gateway_projection_assembler;
mod mcp_gateway_projection_compiler;
mod mcp_gateway_projection_planner;
mod mcp_gateway_projection_set_planner;
mod mcp_route_projection_input_reader;
mod mcp_route_projection_planner;
mod mcp_route_target_projection_compiler;
pub mod persistence;
mod route_target_reader;
mod runtime_http_upstream;
mod vault_gateway_certificate_authority;

#[cfg(test)]
mod gateway_certificate_reconciler_tests;
#[cfg(test)]
mod gateway_replica_recovery_reconciler_tests;
#[cfg(test)]
mod gateway_rollout_reconciler_tests;
#[cfg(test)]
mod gateway_rollout_rollback_compiler_tests;
#[cfg(test)]
mod gateway_route_rollout_compiler_tests;
#[cfg(test)]
mod gateway_snapshot_compiler_tests;

pub use deployment_route_updater::EdgeDeploymentRouteUpdater;
pub use domain_ownership_verifier::{DnsDomainOwnershipVerifier, LocalDomainOwnershipVerifier};
pub use gateway_acknowledgement_projector::EdgeGatewayAcknowledgementProjector;
pub use gateway_certificate_reconciler::{
    GatewayCertificateReconciler, GatewayCertificateReconciliationFailure,
    GatewayCertificateReconciliationReport,
};
pub use gateway_command_queue::FleetGatewayCommandQueue;
pub use gateway_observation_queue::FleetGatewayObservationQueue;
pub use gateway_replica_recovery_reconciler::{
    GatewayReplicaRecoveryReconciler, GatewayReplicaRecoveryReconciliationFailure,
    GatewayReplicaRecoveryReconciliationReport,
};
pub use gateway_rollout_reconciler::{
    GatewayRolloutReconciler, GatewayRolloutReconciliationFailure,
    GatewayRolloutReconciliationReport,
};
pub use gateway_rollout_rollback_compiler::{
    CompileGatewayRolloutRollback, CompiledGatewayRolloutRollback,
    GatewayRollbackMemberSnapshotContext, GatewayRolloutRollbackCompiler,
};
pub use gateway_rollout_rollback_reconciler::{
    GatewayRolloutRollbackReconciler, GatewayRolloutRollbackReconciliationFailure,
    GatewayRolloutRollbackReconciliationReport,
};
pub use gateway_route_rollout_compiler::{
    CompileGatewayRouteRollout, CompiledGatewayRouteRollout, GatewayMemberSnapshotContext,
    GatewayRouteRolloutCompiler,
};
pub use gateway_route_rollout_planner::{GatewayRouteRolloutPlanner, PlanGatewayRouteRollout};
pub use gateway_snapshot_compiler::{
    CompileMcpGatewaySnapshot, CompiledMcpGatewaySnapshot, GatewayDomainClaimVersion,
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    GatewaySnapshotRouteInput,
};
pub use local_gateway_certificate_authority::LocalGatewayCertificateAuthority;
pub use mcp_credential_issuer::{
    IssuedMcpCredential, McpCredentialIssuanceError, McpCredentialIssueRequest, McpCredentialIssuer,
};
pub use mcp_gateway_projection_assembler::McpGatewayProjectionAssembler;
pub use mcp_gateway_projection_compiler::{
    CompiledMcpGatewayProjection, McpGatewayProjectionCompiler,
};
pub use mcp_gateway_projection_planner::{
    McpCredentialProjectionVersion, McpGatewayProjectionPlanner, PlannedMcpGatewayProjection,
};
pub use mcp_gateway_projection_set_planner::{
    McpGatewayIngressRoute, McpGatewayProjectionSetPlanner, McpRouteProjectionVersion,
    PlanMcpGatewayProjectionSet, PlannedMcpGatewayProjectionSet,
};
pub use mcp_route_projection_input_reader::McpRouteProjectionInputReader;
pub use mcp_route_projection_planner::{McpRouteProjectionPlanner, PlanMcpRouteProjection};
pub use mcp_route_target_projection_compiler::{
    McpRouteTargetCandidate, McpRouteTargetProjectionCompiler,
};
pub use route_target_reader::WorkloadRouteTargetReader;
pub use vault_gateway_certificate_authority::VaultGatewayCertificateAuthority;
