use crate::modules::edge::application::McpCredentialMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{McpCredentialId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeMcpCredential {
    pub organization_id: OrganizationId,
    pub credential_id: McpCredentialId,
    pub expected_aggregate_version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RevokeMcpCredential {
    type Output = ApplicationResult<McpCredentialMutationResult>;
}
