use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetReleaseWrite;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct YankAssetRelease {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for YankAssetRelease {
    type Output = ApplicationResult<AssetReleaseWrite>;
}

pub struct YankAssetReleaseHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl YankAssetReleaseHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<YankAssetRelease> for YankAssetReleaseHandler {
    fn execute(
        &self,
        command: YankAssetRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetReleaseWrite>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .yank_release(
                    command.organization_id,
                    command.asset_id,
                    command.asset_release_id,
                    &command.resource_access,
                    command.idempotency_key,
                    command.request_id,
                )
                .await)
        })
    }
}
