use crate::modules::edge::application::McpRoutePolicyApplicationService;
use crate::modules::edge::domain::repositories::McpRoutePolicyWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReviseMcpRoutePolicy {
    pub organization_id: OrganizationId,
    pub route_id: RouteId,
    pub acl: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for ReviseMcpRoutePolicy {
    type Output = ApplicationResult<McpRoutePolicyWrite>;
}

pub struct ReviseMcpRoutePolicyHandler {
    service: Arc<McpRoutePolicyApplicationService>,
}

impl ReviseMcpRoutePolicyHandler {
    pub fn new(service: Arc<McpRoutePolicyApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ReviseMcpRoutePolicy> for ReviseMcpRoutePolicyHandler {
    fn execute(
        &self,
        command: ReviseMcpRoutePolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpRoutePolicyWrite>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .revise(
                    command.organization_id,
                    command.route_id,
                    command.acl,
                    command.idempotency_key,
                    command.request_id,
                    command.requested_at,
                )
                .await)
        })
    }
}
