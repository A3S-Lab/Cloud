use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, HumanTaskId, OrganizationId, PrincipalId};
use crate::modules::workflow::application::HumanTaskMutationResult;
use a3s_boot::Command;
use a3s_form_core::FormInteractionSubmission;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SubmitHumanTask {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
    pub resource_access: ResourceAccessEvaluator,
    pub submission: FormInteractionSubmission,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for SubmitHumanTask {
    type Output = ApplicationResult<HumanTaskMutationResult>;
}
