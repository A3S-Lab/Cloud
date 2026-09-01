use crate::modules::assets::application::{AssetAccess, AssetGitApplicationService};
use crate::modules::assets::domain::AssetGitRpcResponse;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct UploadAssetGitPack {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub access: AssetAccess,
    pub body: Vec<u8>,
}

impl Query for UploadAssetGitPack {
    type Output = ApplicationResult<AssetGitRpcResponse>;
}

pub struct UploadAssetGitPackHandler {
    service: Arc<AssetGitApplicationService>,
}

impl UploadAssetGitPackHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<UploadAssetGitPack> for UploadAssetGitPackHandler {
    fn execute(
        &self,
        query: UploadAssetGitPack,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetGitRpcResponse>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .upload_pack(
                    query.organization_id,
                    query.asset_id,
                    query.body,
                    &query.access,
                )
                .await)
        })
    }
}
