use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetRelease;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListAssetReleases {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListAssetReleases {
    type Output = ApplicationResult<Vec<AssetRelease>>;
}

pub struct ListAssetReleasesHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl ListAssetReleasesHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListAssetReleases> for ListAssetReleasesHandler {
    fn execute(
        &self,
        query: ListAssetReleases,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<AssetRelease>>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .list_releases(
                    query.organization_id,
                    query.asset_id,
                    &query.resource_access,
                )
                .await)
        })
    }
}
