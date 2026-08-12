use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetRelease;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAssetRelease {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAssetRelease {
    type Output = ApplicationResult<AssetRelease>;
}

pub struct GetAssetReleaseHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl GetAssetReleaseHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetAssetRelease> for GetAssetReleaseHandler {
    fn execute(
        &self,
        query: GetAssetRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetRelease>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .get_release(
                    query.organization_id,
                    query.asset_id,
                    query.asset_release_id,
                    &query.resource_access,
                )
                .await)
        })
    }
}
