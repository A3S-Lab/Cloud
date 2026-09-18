use crate::modules::identity::domain::entities::ExternalIdentityLink;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LinkPartnerSubject {
    pub organization_id: OrganizationId,
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub principal_id: PrincipalId,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSubjectLinkMutationResult {
    pub link: ExternalIdentityLink,
    pub replayed: bool,
}

impl Command for LinkPartnerSubject {
    type Output = ApplicationResult<PartnerSubjectLinkMutationResult>;
}
