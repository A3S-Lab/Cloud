pub use a3s_cloud_contracts::NATIVE_CODE_AGENT_PROVIDER_KIND;
use a3s_cloud_contracts::{AgentProviderProfile, AGENT_PROTOCOL_V1};
use serde::{Deserialize, Serialize};

const NATIVE_CODE_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/a3s-code-provider-profile.acl"
));

/// Immutable provider admission evidence bound to one logical Agent execution.
///
/// The canonical ACL remains the source of truth. The repeated scalar values
/// are indexed persistence evidence and must re-derive exactly from that ACL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProviderProfileBinding {
    kind: String,
    revision: String,
    protocol: String,
    native_protocol: String,
    profile_acl: String,
    profile_digest: String,
    capability_digest: String,
}

impl AgentProviderProfileBinding {
    pub fn from_profile(profile: &AgentProviderProfile) -> Result<Self, String> {
        profile.validate()?;
        let binding = Self {
            kind: profile.kind().into(),
            revision: profile.revision().into(),
            protocol: profile.protocol().into(),
            native_protocol: profile.native_protocol().into(),
            profile_acl: profile.canonical_acl().into(),
            profile_digest: profile.digest().into(),
            capability_digest: profile.capability_digest().into(),
        };
        binding.validate()?;
        Ok(binding)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        kind: String,
        revision: String,
        protocol: String,
        native_protocol: String,
        profile_acl: String,
        profile_digest: String,
        capability_digest: String,
    ) -> Result<Self, String> {
        let binding = Self {
            kind,
            revision,
            protocol,
            native_protocol,
            profile_acl,
            profile_digest,
            capability_digest,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn native_code() -> Result<Self, String> {
        let profile = AgentProviderProfile::parse_acl(NATIVE_CODE_PROVIDER_PROFILE_ACL)?;
        if profile.kind() != NATIVE_CODE_AGENT_PROVIDER_KIND
            || profile.native_protocol() != AGENT_PROTOCOL_V1
        {
            return Err("native Code provider profile does not match A3S Code Core".into());
        }
        Self::from_profile(&profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        let profile = AgentProviderProfile::restore(&self.profile_acl, &self.profile_digest)?;
        if self.kind != profile.kind()
            || self.revision != profile.revision()
            || self.protocol != profile.protocol()
            || self.native_protocol != profile.native_protocol()
            || self.capability_digest != profile.capability_digest()
        {
            return Err("Agent provider profile binding changed its canonical ACL evidence".into());
        }
        Ok(())
    }

    pub fn profile(&self) -> Result<AgentProviderProfile, String> {
        AgentProviderProfile::restore(&self.profile_acl, &self.profile_digest)
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn native_protocol(&self) -> &str {
        &self.native_protocol
    }

    pub fn profile_acl(&self) -> &str {
        &self.profile_acl
    }

    pub fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub fn capability_digest(&self) -> &str {
        &self.capability_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AgentProviderCapabilityRequirementsV1, AgentProviderCapabilityV1,
        AGENT_PROVIDER_PROTOCOL_V1,
    };

    #[test]
    fn native_code_binding_is_acl_and_capability_bound() {
        let binding = AgentProviderProfileBinding::native_code().expect("native Code profile");
        assert_eq!(binding.kind(), "a3s.code");
        assert_eq!(binding.revision(), "8.1.0");
        assert_eq!(binding.protocol(), AGENT_PROVIDER_PROTOCOL_V1);
        assert_eq!(binding.native_protocol(), AGENT_PROTOCOL_V1);
        let profile = binding.profile().expect("bound profile");
        profile
            .negotiate(
                &AgentProviderCapabilityRequirementsV1::new(vec![
                    AgentProviderCapabilityV1::Cancellation,
                    AgentProviderCapabilityV1::Recovery,
                ])
                .expect("requirements"),
            )
            .expect("Code capabilities");
    }
}
