use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IMcpCredentialRepository: Send + Sync {
    async fn create_mcp_credential(
        &self,
        credential: McpCredential,
    ) -> Result<McpCredential, RepositoryError>;

    async fn update_mcp_credential(
        &self,
        credential: McpCredential,
        expected_aggregate_version: u64,
    ) -> Result<McpCredential, RepositoryError>;

    async fn find_mcp_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
    ) -> Result<Option<McpCredential>, RepositoryError>;

    async fn list_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpCredential>, RepositoryError>;
}
