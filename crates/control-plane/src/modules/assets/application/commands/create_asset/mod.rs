use crate::modules::assets::application::AssetCatalogApplicationService;
use crate::modules::assets::domain::AssetWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateAsset {
    pub organization_id: OrganizationId,
    pub name: String,
    pub kind: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateAsset {
    type Output = ApplicationResult<AssetWrite>;
}

pub struct CreateAssetHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl CreateAssetHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateAsset> for CreateAssetHandler {
    fn execute(
        &self,
        command: CreateAsset,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetWrite>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .create_asset(
                    command.organization_id,
                    command.name,
                    command.kind,
                    command.idempotency_key,
                    command.request_id,
                )
                .await)
        })
    }
}
