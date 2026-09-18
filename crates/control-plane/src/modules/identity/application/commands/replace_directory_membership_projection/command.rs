use crate::modules::identity::application::DirectoryMembershipProjectionMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReplaceDirectoryMembershipProjection {
    pub organization_id: OrganizationId,
    pub subject_ref: String,
    pub principal_ids: Vec<Uuid>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReplaceDirectoryMembershipProjection {
    type Output = ApplicationResult<DirectoryMembershipProjectionMutationResult>;
}
