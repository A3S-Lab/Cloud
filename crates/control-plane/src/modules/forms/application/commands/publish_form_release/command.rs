use crate::modules::forms::application::FormPublicationMutationResult;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PublishFormRelease {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
    pub resource_access: ResourceAccessEvaluator,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for PublishFormRelease {
    type Output = ApplicationResult<FormPublicationMutationResult>;
}
