use a3s_cloud_contracts::AutomationNormalizedEventV1;
use async_trait::async_trait;

/// Application-owned handoff for an already normalized plugin or Source event.
///
/// A provider consumer may retry a failed handoff, but it never owns
/// normalization, filter storage, invocation persistence, or target execution.
#[async_trait]
pub trait IAutomationNormalizedEventHandler: Send + Sync {
    async fn handle(&self, event: AutomationNormalizedEventV1) -> Result<(), String>;
}
