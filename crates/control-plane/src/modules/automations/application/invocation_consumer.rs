use crate::modules::automations::domain::AutomationInvocationRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use async_trait::async_trait;

/// Target-owner handoff for one already-admitted immutable invocation.
///
/// The handler owns target lookup and execution. Automations only verifies the
/// durable Outbox identity and replays the exact invocation record.
#[async_trait]
pub trait IAutomationInvocationHandler: Send + Sync {
    async fn handle(&self, invocation: AutomationInvocationRecord) -> ApplicationResult<()>;
}
