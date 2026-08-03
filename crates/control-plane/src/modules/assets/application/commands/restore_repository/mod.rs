use crate::modules::assets::application::AssetGitApplicationService;
use crate::modules::assets::domain::{AssetGitBackup, AssetGitRpcResponse};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RestoreAssetGitRepository {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub actor_id: Uuid,
    pub request_id: Uuid,
    pub backup: AssetGitBackup,
}

impl Command for RestoreAssetGitRepository {
    type Output = ApplicationResult<AssetGitRpcResponse>;
}

pub struct RestoreAssetGitRepositoryHandler {
    service: Arc<AssetGitApplicationService>,
}

impl RestoreAssetGitRepositoryHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<RestoreAssetGitRepository> for RestoreAssetGitRepositoryHandler {
    fn execute(
        &self,
        command: RestoreAssetGitRepository,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetGitRpcResponse>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .restore_repository(
                    command.organization_id,
                    command.asset_id,
                    command.actor_id,
                    command.request_id,
                    command.backup,
                )
                .await)
        })
    }
}
