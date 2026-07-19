use crate::modules::edge::domain::DomainClaim;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateDomainClaim {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub pattern: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateDomainClaim {
    type Output = ApplicationResult<CreateDomainClaimResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateDomainClaimResult {
    pub claim: DomainClaim,
    pub replayed: bool,
}
