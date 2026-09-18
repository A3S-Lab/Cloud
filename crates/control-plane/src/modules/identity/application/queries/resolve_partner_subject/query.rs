use crate::modules::identity::domain::entities::ExternalIdentityLink;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResolvePartnerSubject {
    pub organization_id: OrganizationId,
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSubjectLinkView {
    pub link_id: Uuid,
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub principal_id: Uuid,
    pub aggregate_version: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_verified_at: chrono::DateTime<chrono::Utc>,
}

impl From<ExternalIdentityLink> for PartnerSubjectLinkView {
    fn from(link: ExternalIdentityLink) -> Self {
        Self {
            link_id: link.id.as_uuid(),
            provider_key: link.provider_key.as_str().to_owned(),
            issuer: link.issuer.as_str().to_owned(),
            subject: link.subject.as_str().to_owned(),
            principal_id: link.principal_id.as_uuid(),
            aggregate_version: link.aggregate_version,
            created_at: link.created_at,
            last_verified_at: link.last_verified_at,
        }
    }
}

impl Query for ResolvePartnerSubject {
    type Output = ApplicationResult<PartnerSubjectLinkView>;
}
