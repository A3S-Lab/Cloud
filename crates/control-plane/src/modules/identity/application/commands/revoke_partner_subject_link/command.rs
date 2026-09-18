use crate::modules::identity::application::commands::link_partner_subject::PartnerSubjectLinkMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokePartnerSubjectLink {
    pub organization_id: OrganizationId,
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub expected_version: u64,
    pub actor_principal_id: crate::modules::shared_kernel::domain::PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokePartnerSubjectLink {
    type Output = ApplicationResult<PartnerSubjectLinkMutationResult>;
}
