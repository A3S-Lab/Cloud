use crate::modules::forms::application::FormDraftMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateFormDraft {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub name: String,
    pub description: String,
    pub document_json: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateFormDraft {
    type Output = ApplicationResult<FormDraftMutationResult>;
}
