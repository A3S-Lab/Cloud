use crate::modules::edge::domain::repositories::IMcpCredentialLifecycleRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{McpCredentialId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetMcpCredential {
    pub organization_id: OrganizationId,
    pub credential_id: McpCredentialId,
}

impl Query for GetMcpCredential {
    type Output = ApplicationResult<McpCredential>;
}

pub struct GetMcpCredentialHandler {
    credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
}

impl GetMcpCredentialHandler {
    pub fn new(credentials: Arc<dyn IMcpCredentialLifecycleRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<GetMcpCredential> for GetMcpCredentialHandler {
    fn execute(
        &self,
        query: GetMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpCredential>>> {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            match credentials
                .find_mcp_credential(query.organization_id, query.credential_id)
                .await
            {
                Ok(Some(credential)) => Ok(Ok(credential)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "MCP credential not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
