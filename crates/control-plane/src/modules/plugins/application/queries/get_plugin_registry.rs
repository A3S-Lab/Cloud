use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetPluginRegistry {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
}

impl Query for GetPluginRegistry {
    type Output = ApplicationResult<PluginRegistry>;
}

pub struct GetPluginRegistryHandler {
    registries: Arc<dyn IPluginRegistryRepository>,
}

impl GetPluginRegistryHandler {
    pub fn new(registries: Arc<dyn IPluginRegistryRepository>) -> Self {
        Self { registries }
    }
}

impl QueryHandler<GetPluginRegistry> for GetPluginRegistryHandler {
    fn execute(
        &self,
        query: GetPluginRegistry,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginRegistry>>> {
        let registries = Arc::clone(&self.registries);
        Box::pin(async move {
            match registries
                .find(query.organization_id, query.registry_id)
                .await
            {
                Ok(Some(registry)) => Ok(Ok(registry)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "plugin registry not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
