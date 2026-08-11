use crate::modules::edge::application::McpRoutePolicyApplicationService;
use crate::modules::edge::domain::repositories::McpRoutePolicyWrite;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateMcpRoutePolicy {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub acl: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateMcpRoutePolicy {
    type Output = ApplicationResult<McpRoutePolicyWrite>;
}

pub struct CreateMcpRoutePolicyHandler {
    service: Arc<McpRoutePolicyApplicationService>,
}

impl CreateMcpRoutePolicyHandler {
    pub fn new(service: Arc<McpRoutePolicyApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateMcpRoutePolicy> for CreateMcpRoutePolicyHandler {
    fn execute(
        &self,
        command: CreateMcpRoutePolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpRoutePolicyWrite>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .create(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.acl,
                    command.idempotency_key,
                    command.request_id,
                    command.requested_at,
                )
                .await)
        })
    }
}
