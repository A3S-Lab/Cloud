use crate::modules::edge::domain::events::McpRoutePolicyMutationKind;
use crate::modules::edge::domain::{McpRoutePolicy, McpRoutePolicyDocument};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, IdempotencyRequest, OrganizationId,
    ProjectId, RepositoryError, RouteId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY: usize = 1_000;

#[derive(Debug, Clone)]
pub struct MutateMcpRoutePolicyWrite {
    pub document: McpRoutePolicyDocument,
    pub kind: McpRoutePolicyMutationKind,
    pub idempotency: IdempotencyRequest,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl MutateMcpRoutePolicyWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
            || (self.kind == McpRoutePolicyMutationKind::Create
                && self.document.policy_revision() != 1)
        {
            return Err("MCP route policy mutation request is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRoutePolicyWrite {
    pub policy: McpRoutePolicy,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpRoutePolicyWriteSnapshot {
    pub canonical_acl: String,
    pub policy_digest: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&McpRoutePolicy> for McpRoutePolicyWriteSnapshot {
    fn from(policy: &McpRoutePolicy) -> Self {
        Self {
            canonical_acl: policy.canonical_acl().into(),
            policy_digest: policy.policy_digest().to_string(),
            created_at: policy.created_at(),
            updated_at: policy.updated_at(),
        }
    }
}

#[async_trait]
pub trait IMcpRoutePolicyRepository: Send + Sync {
    /// Apply one caller-owned create or revision request atomically with its
    /// idempotency response, audit record, and changed-only Outbox event.
    async fn mutate_mcp_route_policy(
        &self,
        write: MutateMcpRoutePolicyWrite,
    ) -> Result<McpRoutePolicyWrite, RepositoryError>;

    async fn find_mcp_route_policy(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Option<McpRoutePolicy>, RepositoryError>;

    async fn list_mcp_route_policies(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError>;

    /// Reads the complete active desired-route set for one exact logical
    /// Gateway scope. Implementations must fail rather than truncate when the
    /// fixed projection bound is exceeded.
    async fn list_active_mcp_route_policies_for_gateway(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        gateway_scope_id: GatewayScopeId,
        active_at: DateTime<Utc>,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError>;
}
