use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeSet;

pub(crate) const MAX_MCP_CREDENTIAL_RESOLUTION_BATCH: usize = 10_000;

pub(crate) fn validate_mcp_credential_resolution(
    credential_ids: &[McpCredentialId],
) -> Result<(), RepositoryError> {
    if credential_ids.len() > MAX_MCP_CREDENTIAL_RESOLUTION_BATCH
        || credential_ids
            .iter()
            .any(|credential_id| credential_id.as_uuid().is_nil())
        || credential_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != credential_ids.len()
    {
        return Err(RepositoryError::Conflict(
            "MCP credential resolution requires at most 10000 unique non-nil identities".into(),
        ));
    }
    Ok(())
}

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

    /// Resolves only the requested credential identities within one exact
    /// tenant scope. Missing or cross-scope identities are intentionally
    /// omitted so callers can apply tenant non-disclosure.
    async fn resolve_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_ids: &[McpCredentialId],
    ) -> Result<Vec<McpCredential>, RepositoryError>;
}
