use crate::modules::assets::application::AssetGitApplicationService;
use crate::modules::assets::domain::AssetGitRpcResponse;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReceiveAssetGitPack {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub actor_id: Uuid,
    pub request_id: Uuid,
    pub body: Vec<u8>,
}

impl Command for ReceiveAssetGitPack {
    type Output = ApplicationResult<AssetGitRpcResponse>;
}

pub struct ReceiveAssetGitPackHandler {
    service: Arc<AssetGitApplicationService>,
}

impl ReceiveAssetGitPackHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ReceiveAssetGitPack> for ReceiveAssetGitPackHandler {
    fn execute(
        &self,
        command: ReceiveAssetGitPack,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetGitRpcResponse>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .receive_pack(
                    command.organization_id,
                    command.asset_id,
                    command.actor_id,
                    command.request_id,
                    command.body,
                )
                .await)
        })
    }
}
