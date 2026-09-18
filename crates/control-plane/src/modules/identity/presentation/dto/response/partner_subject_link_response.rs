use crate::modules::identity::application::commands::link_partner_subject::PartnerSubjectLinkMutationResult;
use crate::modules::identity::application::queries::resolve_partner_subject::PartnerSubjectLinkView;
use crate::modules::identity::domain::entities::ExternalIdentityLink;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSubjectLinkResponse {
    pub link_id: Uuid,
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub principal_id: Uuid,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub last_verified_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<ExternalIdentityLink> for PartnerSubjectLinkResponse {
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
            revoked_at: link.revoked_at,
        }
    }
}

impl From<PartnerSubjectLinkView> for PartnerSubjectLinkResponse {
    fn from(view: PartnerSubjectLinkView) -> Self {
        Self {
            link_id: view.link_id,
            provider_key: view.provider_key,
            issuer: view.issuer,
            subject: view.subject,
            principal_id: view.principal_id,
            aggregate_version: view.aggregate_version,
            created_at: view.created_at,
            last_verified_at: view.last_verified_at,
            revoked_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSubjectLinkMutationResponse {
    #[serde(flatten)]
    pub link: PartnerSubjectLinkResponse,
    pub replayed: bool,
}

impl From<PartnerSubjectLinkMutationResult> for PartnerSubjectLinkMutationResponse {
    fn from(result: PartnerSubjectLinkMutationResult) -> Self {
        Self {
            link: result.link.into(),
            replayed: result.replayed,
        }
    }
}
