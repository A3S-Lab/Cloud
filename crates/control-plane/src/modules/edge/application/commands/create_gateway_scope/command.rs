use crate::modules::edge::domain::GatewayScope;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, OrganizationId, ProjectId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateGatewayScope {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub node_id: NodeId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateGatewayScope {
    type Output = ApplicationResult<CreateGatewayScopeResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateGatewayScopeResult {
    pub scope: GatewayScope,
    pub replayed: bool,
}
