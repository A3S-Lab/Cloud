//! Outbound node control and Runtime provider boundary.

mod agent;
mod config;
mod control_plane;
mod docker;
mod executor;
mod gateway;
mod gateway_certificate;
mod identity;
mod journal;
mod state_file;

pub use config::{
    ConfigError, ControlPlaneConfig, DockerConfig, GatewayControlConfig, NodeAgentConfig,
    NodeConfig,
};
pub use control_plane::{
    GatewayCertificateSigningTransport, NodeControlClient, NodeControlClientError,
    NodeControlTransport,
};
pub use docker::DockerRuntimeDriver;
pub use executor::{CommandExecutionError, CommandExecutor};
pub use gateway::{
    DurableGatewaySnapshotInstaller, GatewaySnapshotInstallError, GatewaySnapshotInstallOutcome,
    GatewaySnapshotInstaller,
};
pub use identity::{
    EnrolledNodeIdentity, FileNodeIdentityStore, IdentityStoreError, NodeIdentityState,
    PendingNodeIdentity,
};
pub use journal::{CommandJournalError, FileCommandJournal, JournalDecision};

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
pub use agent::{
    run_node_agent, NodeAgentError, NodeAgentSession, NodeRuntimeBinding, NodeRuntimeProvider,
};
