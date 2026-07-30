use crate::modules::edge::application::mcp_credential_lifecycle::exact_credential;
use crate::modules::edge::domain::repositories::IMcpCredentialAuthorityRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetMcpCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
}

impl Query for GetMcpCredential {
    type Output = ApplicationResult<McpCredential>;
}

pub struct GetMcpCredentialHandler {
    credentials: Arc<dyn IMcpCredentialAuthorityRepository>,
}

impl GetMcpCredentialHandler {
    pub fn new(credentials: Arc<dyn IMcpCredentialAuthorityRepository>) -> Self {
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
            let credential = match credentials
                .find_mcp_credential(query.organization_id, query.credential_id)
                .await
            {
                Ok(credential) => credential,
                Err(error) => return Ok(Err(ApplicationError::from(error))),
            };
            Ok(exact_credential(
                credential,
                query.project_id,
                query.environment_id,
            ))
        })
    }
}
