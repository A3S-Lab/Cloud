use super::AgentExecutionProvider;
use crate::modules::agents::domain::AgentProviderProfileBinding;
use std::sync::Arc;

/// Closed resolver for provider implementations admitted by this Cloud build.
///
/// Callers select only a bounded provider kind. Recovery resolves the exact
/// persisted profile, so a process configuration change cannot silently move
/// an existing execution to another provider revision or capability set.
pub trait AgentExecutionProviderRegistry: Send + Sync {
    fn provider_by_kind(&self, kind: &str) -> Result<Arc<dyn AgentExecutionProvider>, String>;

    fn provider_for_profile(
        &self,
        profile: &AgentProviderProfileBinding,
    ) -> Result<Arc<dyn AgentExecutionProvider>, String> {
        profile.validate()?;
        let provider = self.provider_by_kind(profile.kind())?;
        if provider.profile() != profile {
            return Err(format!(
                "Agent provider {:?} does not match the persisted immutable profile",
                profile.kind()
            ));
        }
        Ok(provider)
    }
}
