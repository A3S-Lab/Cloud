use super::{
    AutomationWebhookLifecycleService, ChangeAuthorizedAutomationWebhookEndpoint,
    CreateAuthorizedAutomationWebhookEndpoint,
};
use crate::modules::automations::domain::AutomationWebhookEndpointRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;

impl Command for CreateAuthorizedAutomationWebhookEndpoint {
    type Output = ApplicationResult<AutomationWebhookEndpointRecord>;
}

pub struct CreateAuthorizedAutomationWebhookEndpointHandler {
    service: Arc<AutomationWebhookLifecycleService>,
}

impl CreateAuthorizedAutomationWebhookEndpointHandler {
    pub fn new(service: Arc<AutomationWebhookLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateAuthorizedAutomationWebhookEndpoint>
    for CreateAuthorizedAutomationWebhookEndpointHandler
{
    fn execute(
        &self,
        command: CreateAuthorizedAutomationWebhookEndpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AutomationWebhookEndpointRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.create_endpoint(command).await) })
    }
}

impl Command for ChangeAuthorizedAutomationWebhookEndpoint {
    type Output = ApplicationResult<AutomationWebhookEndpointRecord>;
}

pub struct ChangeAuthorizedAutomationWebhookEndpointHandler {
    service: Arc<AutomationWebhookLifecycleService>,
}

impl ChangeAuthorizedAutomationWebhookEndpointHandler {
    pub fn new(service: Arc<AutomationWebhookLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ChangeAuthorizedAutomationWebhookEndpoint>
    for ChangeAuthorizedAutomationWebhookEndpointHandler
{
    fn execute(
        &self,
        command: ChangeAuthorizedAutomationWebhookEndpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AutomationWebhookEndpointRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.change_endpoint(command).await) })
    }
}
