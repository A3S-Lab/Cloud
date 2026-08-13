use crate::modules::identity::application::MembershipInvitationMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateMembershipInvitation {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub role: String,
    pub expires_at: DateTime<Utc>,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateMembershipInvitation {
    type Output = ApplicationResult<MembershipInvitationMutationResult>;
}
