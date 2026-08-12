use crate::modules::assets::application::McpServiceProfileApplicationService;
use crate::modules::assets::domain::McpServiceProfileWrite;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BindMcpServiceProfile {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub resource_access: ResourceAccessEvaluator,
    pub acl: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for BindMcpServiceProfile {
    type Output = ApplicationResult<McpServiceProfileWrite>;
}

pub struct BindMcpServiceProfileHandler {
    service: Arc<McpServiceProfileApplicationService>,
}

impl BindMcpServiceProfileHandler {
    pub fn new(service: Arc<McpServiceProfileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<BindMcpServiceProfile> for BindMcpServiceProfileHandler {
    fn execute(
        &self,
        command: BindMcpServiceProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpServiceProfileWrite>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .bind(
                    command.organization_id,
                    command.asset_id,
                    command.asset_release_id,
                    &command.resource_access,
                    command.acl,
                    command.idempotency_key,
                    command.request_id,
                )
                .await)
        })
    }
}
