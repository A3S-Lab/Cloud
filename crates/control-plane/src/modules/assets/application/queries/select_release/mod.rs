use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetRelease;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SelectAssetRelease {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub requested_version: Option<String>,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for SelectAssetRelease {
    type Output = ApplicationResult<AssetRelease>;
}

pub struct SelectAssetReleaseHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl SelectAssetReleaseHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<SelectAssetRelease> for SelectAssetReleaseHandler {
    fn execute(
        &self,
        query: SelectAssetRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetRelease>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .select_release(
                    query.organization_id,
                    query.asset_id,
                    query.requested_version,
                    &query.resource_access,
                )
                .await)
        })
    }
}
