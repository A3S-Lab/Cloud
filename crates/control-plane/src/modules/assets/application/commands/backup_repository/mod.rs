use crate::modules::assets::application::AssetGitApplicationService;
use crate::modules::assets::domain::AssetGitBackup;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BackupAssetGitRepository {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub actor_id: Uuid,
    pub request_id: Uuid,
}

impl Command for BackupAssetGitRepository {
    type Output = ApplicationResult<AssetGitBackup>;
}

pub struct BackupAssetGitRepositoryHandler {
    service: Arc<AssetGitApplicationService>,
}

impl BackupAssetGitRepositoryHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<BackupAssetGitRepository> for BackupAssetGitRepositoryHandler {
    fn execute(
        &self,
        command: BackupAssetGitRepository,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetGitBackup>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .backup_repository(
                    command.organization_id,
                    command.asset_id,
                    command.actor_id,
                    command.request_id,
                )
                .await)
        })
    }
}
