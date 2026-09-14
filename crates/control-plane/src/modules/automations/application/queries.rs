use super::{
    AutomationDefinitionQueryService, AutomationWebhookLifecycleService,
    GetAuthorizedAutomationDefinition, GetAuthorizedAutomationRevision,
    GetAuthorizedAutomationWebhookEndpoint, ListAuthorizedAutomationDefinitions,
};
use crate::modules::automations::domain::{
    AutomationDefinitionRecord, AutomationWebhookEndpointRecord,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_cloud_contracts::AutomationRevisionV1;
use a3s_boot::{Query, QueryHandler};
use std::sync::Arc;

impl Query for GetAuthorizedAutomationWebhookEndpoint {
    type Output = ApplicationResult<AutomationWebhookEndpointRecord>;
}

pub struct GetAuthorizedAutomationWebhookEndpointHandler {
    service: Arc<AutomationWebhookLifecycleService>,
}

impl GetAuthorizedAutomationWebhookEndpointHandler {
    pub fn new(service: Arc<AutomationWebhookLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetAuthorizedAutomationWebhookEndpoint>
    for GetAuthorizedAutomationWebhookEndpointHandler
{
    fn execute(
        &self,
        query: GetAuthorizedAutomationWebhookEndpoint,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AutomationWebhookEndpointRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_endpoint(query).await) })
    }
}

impl Query for GetAuthorizedAutomationDefinition {
    type Output = ApplicationResult<AutomationDefinitionRecord>;
}

pub struct GetAuthorizedAutomationDefinitionHandler {
    service: Arc<AutomationDefinitionQueryService>,
}

impl GetAuthorizedAutomationDefinitionHandler {
    pub fn new(service: Arc<AutomationDefinitionQueryService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetAuthorizedAutomationDefinition> for GetAuthorizedAutomationDefinitionHandler {
    fn execute(
        &self,
        query: GetAuthorizedAutomationDefinition,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AutomationDefinitionRecord>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get(query).await) })
    }
}

impl Query for ListAuthorizedAutomationDefinitions {
    type Output = ApplicationResult<Vec<AutomationDefinitionRecord>>;
}

pub struct ListAuthorizedAutomationDefinitionsHandler {
    service: Arc<AutomationDefinitionQueryService>,
}

impl ListAuthorizedAutomationDefinitionsHandler {
    pub fn new(service: Arc<AutomationDefinitionQueryService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListAuthorizedAutomationDefinitions>
    for ListAuthorizedAutomationDefinitionsHandler
{
    fn execute(
        &self,
        query: ListAuthorizedAutomationDefinitions,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AutomationDefinitionRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list(query).await) })
    }
}

impl Query for GetAuthorizedAutomationRevision {
    type Output = ApplicationResult<AutomationRevisionV1>;
}

pub struct GetAuthorizedAutomationRevisionHandler {
    service: Arc<AutomationDefinitionQueryService>,
}

impl GetAuthorizedAutomationRevisionHandler {
    pub fn new(service: Arc<AutomationDefinitionQueryService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetAuthorizedAutomationRevision> for GetAuthorizedAutomationRevisionHandler {
    fn execute(
        &self,
        query: GetAuthorizedAutomationRevision,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AutomationRevisionV1>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_revision(query).await) })
    }
}
