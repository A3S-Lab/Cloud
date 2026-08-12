use crate::modules::forms::application::FormDraftMutationResult;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReviseFormDraft {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
    pub resource_access: ResourceAccessEvaluator,
    pub expected_version: u64,
    pub name: String,
    pub description: String,
    pub document_json: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReviseFormDraft {
    type Output = ApplicationResult<FormDraftMutationResult>;
}
