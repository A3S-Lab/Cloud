use crate::modules::assets::application::{AssetAccess, AssetCatalogApplicationService};
use crate::modules::assets::domain::AssetReleaseWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateAssetRelease {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub access: AssetAccess,
    pub version: String,
    pub commit_sha: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateAssetRelease {
    type Output = ApplicationResult<AssetReleaseWrite>;
}

pub struct CreateAssetReleaseHandler {
    service: Arc<AssetCatalogApplicationService>,
}

impl CreateAssetReleaseHandler {
    pub fn new(service: Arc<AssetCatalogApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateAssetRelease> for CreateAssetReleaseHandler {
    fn execute(
        &self,
        command: CreateAssetRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetReleaseWrite>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .create_release(
                    command.organization_id,
                    command.asset_id,
                    &command.access,
                    command.version,
                    command.commit_sha,
                    command.idempotency_key,
                    command.request_id,
                )
                .await)
        })
    }
}
