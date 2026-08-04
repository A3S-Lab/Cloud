use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ArchiveAsset {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ArchiveAsset {
    type Output = ApplicationResult<AssetWrite>;
}

pub struct ArchiveAssetHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl ArchiveAssetHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ArchiveAsset> for ArchiveAssetHandler {
    fn execute(
        &self,
        command: ArchiveAsset,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetWrite>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .archive_asset(
                    command.organization_id,
                    command.asset_id,
                    command.idempotency_key,
                    command.request_id,
                )
                .await)
        })
    }
}
