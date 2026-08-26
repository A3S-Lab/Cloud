use crate::modules::agents::domain::{AgentExecutionProvider, AgentProviderProfileBinding};
use a3s_cloud_contracts::AgentProviderProfile;

const REFERENCE_ECHO_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));

/// Deterministic non-Code reference provider used by the shared conformance
/// suite. It speaks only the common provider protocol and owns no Cloud state.
#[derive(Debug, Clone)]
pub struct ReferenceEchoAgentExecutionProvider {
    profile: AgentProviderProfileBinding,
}

impl ReferenceEchoAgentExecutionProvider {
    pub fn new() -> Result<Self, String> {
        let profile = AgentProviderProfile::parse_acl(REFERENCE_ECHO_PROVIDER_PROFILE_ACL)?;
        if profile.kind() != "reference.echo" {
            return Err("reference Echo provider profile kind changed".into());
        }
        Ok(Self {
            profile: AgentProviderProfileBinding::from_profile(&profile)?,
        })
    }
}

impl AgentExecutionProvider for ReferenceEchoAgentExecutionProvider {
    fn profile(&self) -> &AgentProviderProfileBinding {
        &self.profile
    }
}
