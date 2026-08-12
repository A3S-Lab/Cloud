use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{HumanTaskId, OrganizationId, PrincipalId};
use crate::modules::workflow::application::HumanTaskMutationResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanTaskAssignmentAction {
    Claim,
    Release,
}

impl HumanTaskAssignmentAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::Release => "release",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangeHumanTaskAssignment {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
    pub resource_access: ResourceAccessEvaluator,
    pub action: HumanTaskAssignmentAction,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for ChangeHumanTaskAssignment {
    type Output = ApplicationResult<HumanTaskMutationResult>;
}
