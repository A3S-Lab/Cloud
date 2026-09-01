use crate::modules::assets::application::{AssetAccess, AssetCatalogApplicationService};
use crate::modules::assets::domain::Asset;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAsset {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub access: AssetAccess,
}

impl Query for GetAsset {
    type Output = ApplicationResult<Asset>;
}

pub struct GetAssetHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl GetAssetHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetAsset> for GetAssetHandler {
    fn execute(
        &self,
        query: GetAsset,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Asset>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .get_asset(query.organization_id, query.asset_id, &query.access)
                .await)
        })
    }
}
