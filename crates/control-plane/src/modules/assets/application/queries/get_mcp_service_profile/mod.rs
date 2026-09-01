use crate::modules::assets::application::{AssetAccess, McpServiceProfileApplicationService};
use crate::modules::assets::domain::McpServiceProfileBinding;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetMcpServiceProfile {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub access: AssetAccess,
}

impl Query for GetMcpServiceProfile {
    type Output = ApplicationResult<McpServiceProfileBinding>;
}

pub struct GetMcpServiceProfileHandler {
    service: Arc<McpServiceProfileApplicationService>,
}

impl GetMcpServiceProfileHandler {
    pub fn new(service: Arc<McpServiceProfileApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetMcpServiceProfile> for GetMcpServiceProfileHandler {
    fn execute(
        &self,
        query: GetMcpServiceProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpServiceProfileBinding>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .get(
                    query.organization_id,
                    query.asset_id,
                    query.asset_release_id,
                    &query.access,
                )
                .await)
        })
    }
}
