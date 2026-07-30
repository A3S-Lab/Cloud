use crate::modules::edge::domain::repositories::IMcpCredentialAuthorityRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListMcpCredentials {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListMcpCredentials {
    type Output = ApplicationResult<Vec<McpCredential>>;
}

pub struct ListMcpCredentialsHandler {
    credentials: Arc<dyn IMcpCredentialAuthorityRepository>,
}

impl ListMcpCredentialsHandler {
    pub fn new(credentials: Arc<dyn IMcpCredentialAuthorityRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<ListMcpCredentials> for ListMcpCredentialsHandler {
    fn execute(
        &self,
        query: ListMcpCredentials,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<McpCredential>>>> {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            Ok(credentials
                .list_mcp_credentials(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
