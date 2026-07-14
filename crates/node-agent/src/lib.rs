//! Outbound node control and Runtime provider boundary.

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
