use crate::modules::assets::application::AssetGitApplicationService;
use crate::modules::assets::domain::AssetGitService;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AdvertiseAssetGitRepository {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub service: AssetGitService,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for AdvertiseAssetGitRepository {
    type Output = ApplicationResult<Vec<u8>>;
}

pub struct AdvertiseAssetGitRepositoryHandler {
    service: Arc<AssetGitApplicationService>,
}

impl AdvertiseAssetGitRepositoryHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<AdvertiseAssetGitRepository> for AdvertiseAssetGitRepositoryHandler {
    fn execute(
        &self,
        query: AdvertiseAssetGitRepository,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<u8>>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .advertise(
                    query.organization_id,
                    query.asset_id,
                    query.service,
                    &query.resource_access,
                )
                .await)
        })
    }
}
