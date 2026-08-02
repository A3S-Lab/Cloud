use serde::{Deserialize, Serialize};

/// Fleet-owned request for reading the canonical A3S Use Plugin Host contract.
///
/// The response is the upstream [`a3s_use_core::PluginHostCapabilities`] type;
/// Cloud does not define or persist a parallel capability schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePluginHostCapabilitiesRequest {
    pub schema: String,
    pub generation: u64,
}

impl NodePluginHostCapabilitiesRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.plugin-host-capabilities-request.v1";

    pub fn new(generation: u64) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            generation,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Plugin Host capabilities request schema {:?}",
                self.schema
            ));
        }
        if self.generation == 0 {
            return Err("Plugin Host capabilities request generation must be positive".into());
        }
        Ok(())
    }
}
