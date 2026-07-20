mod deployment_route_updater;
mod domain_ownership_verifier;
mod gateway_acknowledgement_projector;
mod gateway_certificate_reconciler;
mod gateway_command_queue;
mod gateway_snapshot_compiler;
mod local_gateway_certificate_authority;
pub mod persistence;
mod route_target_reader;
mod vault_gateway_certificate_authority;

#[cfg(test)]
mod gateway_certificate_reconciler_tests;

pub use deployment_route_updater::EdgeDeploymentRouteUpdater;
pub use domain_ownership_verifier::{DnsDomainOwnershipVerifier, LocalDomainOwnershipVerifier};
pub use gateway_acknowledgement_projector::EdgeGatewayAcknowledgementProjector;
pub use gateway_certificate_reconciler::{
    GatewayCertificateReconciler, GatewayCertificateReconciliationFailure,
    GatewayCertificateReconciliationReport,
};
pub use gateway_command_queue::FleetGatewayCommandQueue;
pub use gateway_snapshot_compiler::{GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig};
pub use local_gateway_certificate_authority::LocalGatewayCertificateAuthority;
pub use route_target_reader::WorkloadRouteTargetReader;
pub use vault_gateway_certificate_authority::VaultGatewayCertificateAuthority;
