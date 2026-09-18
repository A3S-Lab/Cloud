use crate::modules::identity::application::DirectoryResourceGrantMutationResult;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateDirectoryResourceGrant {
    pub organization_id: OrganizationId,
    pub subject_ref: String,
    pub scope: ResourceGrantScope,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateDirectoryResourceGrant {
    type Output = ApplicationResult<DirectoryResourceGrantMutationResult>;
}
