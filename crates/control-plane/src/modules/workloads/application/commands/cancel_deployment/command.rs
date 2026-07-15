use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{DeploymentId, OrganizationId};
use crate::modules::workloads::domain::entities::Deployment;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CancelDeployment {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelDeployment {
    type Output = ApplicationResult<CancelDeploymentResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelDeploymentResult {
    pub deployment: Deployment,
    pub replayed: bool,
}
