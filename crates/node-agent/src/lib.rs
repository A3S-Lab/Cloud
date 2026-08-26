//! Outbound node control and Runtime provider boundary.

mod agent;
mod agent_provider_event_shipper;
mod agent_provider_harness;
mod artifact;
mod box_build;
#[cfg(target_os = "linux")]
mod box_runtime;
mod code_event_shipper;
mod code_harness;
mod config;
mod control_plane;
mod durable_cell_operator;
mod executor;
mod gateway;
mod gateway_certificate;
mod identity;
mod journal;
mod log_shipper;
mod outbound_batch;
mod plugin_host;
mod resource_claim;
mod resource_inventory;
mod secret;
mod state_file;

pub use agent_provider_event_shipper::AgentProviderEventShippingError;
pub use artifact::{
    DownloadedNodeArtifact, LocalArtifactReader, NodeArtifactError, NodeArtifactManager,
    NodeArtifactTransport,
};
#[cfg(target_os = "linux")]
pub use box_runtime::build_box_runtime_provider;
pub use code_event_shipper::CodeEventShippingError;
pub use config::{
    ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation, BoxRuntimeSevSnpConfig,
    BoxRuntimeSevSnpGeneration, ConfigError, ControlPlaneConfig, GatewayControlConfig,
    LogShippingConfig, NodeAgentConfig, NodeConfig,
};
pub use control_plane::{
    GatewayCertificateSigningTransport, NodeControlClient, NodeControlClientError,
    NodeControlTransport,
};
pub use executor::{CommandExecutionError, CommandExecutor};
pub use gateway::{
    DurableGatewaySnapshotInstaller, GatewaySnapshotInstallError, GatewaySnapshotInstallOutcome,
    GatewaySnapshotInstaller, GatewaySnapshotObservationOutcome,
};
pub use identity::{
    EnrolledNodeIdentity, FileNodeIdentityStore, IdentityStoreError, NodeIdentityState,
    PendingNodeIdentity,
};
pub use journal::{CommandJournalError, FileCommandJournal, JournalDecision, RuntimeLogTarget};
pub use log_shipper::LogShippingError;
pub use resource_inventory::{NodeResourceInventoryAuthority, ResourceInventoryError};
pub use secret::{NodeSecretTransport, SecretMaterial};

use a3s_runtime::ProviderId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentIdentity {
    pub node_name: String,
    pub provider_id: ProviderId,
}

impl NodeAgentIdentity {
    pub fn new(node_name: impl Into<String>, provider_id: ProviderId) -> Result<Self, String> {
        let node_name = node_name.into();
        if node_name.trim().is_empty() || node_name.len() > 255 {
            return Err("node name must be a bounded nonempty value".into());
        }
        Ok(Self {
            node_name,
            provider_id,
        })
    }
}
pub use agent::{run_node_agent, NodeAgentError, NodeAgentSession, NodeRuntimeProvider};
