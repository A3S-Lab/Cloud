mod webhook_admission;

#[cfg(test)]
mod tests;

pub use crate::modules::automations::domain::EndpointLifecycleAction;
pub use webhook_admission::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint,
};
