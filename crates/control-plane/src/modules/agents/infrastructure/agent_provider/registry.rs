use super::{NativeCodeAgentExecutionProvider, ReferenceEchoAgentExecutionProvider};
use crate::modules::agents::domain::{AgentExecutionProvider, AgentExecutionProviderRegistry};
use a3s_cloud_contracts::{AgentProviderCapabilityRequirementsV1, AgentProviderCapabilityV1};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct BuiltInAgentExecutionProviderRegistry {
    providers: BTreeMap<String, Arc<dyn AgentExecutionProvider>>,
}

impl BuiltInAgentExecutionProviderRegistry {
    pub fn new() -> Result<Self, String> {
        Self::from_providers(vec![
            Arc::new(NativeCodeAgentExecutionProvider::new()?),
            Arc::new(ReferenceEchoAgentExecutionProvider::new()?),
        ])
    }

    pub fn from_providers(providers: Vec<Arc<dyn AgentExecutionProvider>>) -> Result<Self, String> {
        let requirements = AgentProviderCapabilityRequirementsV1::new(vec![
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::EventPages,
        ])?;
        let mut registered = BTreeMap::new();
        for provider in providers {
            provider.profile().validate()?;
            provider.negotiate(&requirements)?;
            let kind = provider.profile().kind().to_owned();
            if registered.insert(kind.clone(), provider).is_some() {
                return Err(format!(
                    "Agent provider kind {kind:?} is registered more than once"
                ));
            }
        }
        if registered.is_empty() {
            return Err("Agent provider registry cannot be empty".into());
        }
        Ok(Self {
            providers: registered,
        })
    }
}

impl AgentExecutionProviderRegistry for BuiltInAgentExecutionProviderRegistry {
    fn provider_by_kind(&self, kind: &str) -> Result<Arc<dyn AgentExecutionProvider>, String> {
        self.providers
            .get(kind)
            .cloned()
            .ok_or_else(|| format!("Agent provider kind {kind:?} is not supported"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_resolve_only_their_exact_immutable_profiles() {
        let registry = BuiltInAgentExecutionProviderRegistry::new().expect("provider registry");
        let code = registry
            .provider_by_kind("a3s.code")
            .expect("native Code provider");
        let reference = registry
            .provider_by_kind("reference.echo")
            .expect("reference provider");
        assert_ne!(code.profile(), reference.profile());
        assert!(registry.provider_for_profile(reference.profile()).is_ok());
        assert!(registry.provider_by_kind("unknown.provider").is_err());
    }
}
