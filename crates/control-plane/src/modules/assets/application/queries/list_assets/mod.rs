use crate::modules::assets::application::{AssetAccess, AssetCatalogApplicationService};
use crate::modules::assets::domain::Asset;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListAssets {
    pub organization_id: OrganizationId,
    pub access: AssetAccess,
}

impl Query for ListAssets {
    type Output = ApplicationResult<Vec<Asset>>;
}

pub struct ListAssetsHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl ListAssetsHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListAssets> for ListAssetsHandler {
    fn execute(
        &self,
        query: ListAssets,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Asset>>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .list_assets(query.organization_id, &query.access)
                .await)
        })
    }
}
