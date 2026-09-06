use crate::modules::automations::domain::{
    AutomationWebhookEndpointRecord, IAutomationWebhookRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use std::sync::Arc;
use uuid::Uuid;

/// The complete tenant scope required to resolve an opaque webhook key.
///
/// Keeping the three identifiers together prevents a transport adapter from
/// accidentally widening an endpoint lookup to organization-only matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomationWebhookEndpointScope {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
}

impl AutomationWebhookEndpointScope {
    fn validate(self) -> ApplicationResult<Self> {
        if self.organization_id.is_nil() || self.project_id.is_nil() || self.environment_id.is_nil()
        {
            return Err(ApplicationError::Invalid(
                "Automation webhook endpoint scope identifiers must not be nil".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct ResolveAutomationWebhookEndpoint {
    pub scope: AutomationWebhookEndpointScope,
    pub endpoint_key: String,
}

/// Application query boundary used by a future signed transport adapter.
///
/// The repository remains the persistence authority, but presentation code
/// must resolve through this service so the complete tenant scope and restored
/// endpoint invariant are checked before request capture or signature work.
#[derive(Clone)]
pub struct AutomationWebhookEndpointQueryService {
    repository: Arc<dyn IAutomationWebhookRepository>,
}

impl AutomationWebhookEndpointQueryService {
    pub fn new(repository: Arc<dyn IAutomationWebhookRepository>) -> Self {
        Self { repository }
    }

    pub async fn resolve(
        &self,
        query: ResolveAutomationWebhookEndpoint,
    ) -> ApplicationResult<Option<AutomationWebhookEndpointRecord>> {
        let scope = query.scope.validate()?;
        let record = self
            .repository
            .find_endpoint_by_key(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                &query.endpoint_key,
            )
            .await
            .map_err(ApplicationError::from)?;

        if let Some(record) = &record {
            let expected_scope = (
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                query.endpoint_key.clone(),
            );
            if record.scope_key() != expected_scope {
                return Err(ApplicationError::Internal(
                    "Automation webhook endpoint repository returned a scope-drifting record"
                        .into(),
                ));
            }
            record.validate().map_err(ApplicationError::Internal)?;
        }

        Ok(record)
    }
}
