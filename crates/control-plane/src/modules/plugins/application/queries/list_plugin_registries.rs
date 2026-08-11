use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListPluginRegistries {
    pub organization_id: OrganizationId,
}

impl Query for ListPluginRegistries {
    type Output = ApplicationResult<Vec<PluginRegistry>>;
}

pub struct ListPluginRegistriesHandler {
    registries: Arc<dyn IPluginRegistryRepository>,
}

impl ListPluginRegistriesHandler {
    pub fn new(registries: Arc<dyn IPluginRegistryRepository>) -> Self {
        Self { registries }
    }
}

impl QueryHandler<ListPluginRegistries> for ListPluginRegistriesHandler {
    fn execute(
        &self,
        query: ListPluginRegistries,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<PluginRegistry>>>>
    {
        let registries = Arc::clone(&self.registries);
        Box::pin(async move {
            Ok(registries
                .list(query.organization_id)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
