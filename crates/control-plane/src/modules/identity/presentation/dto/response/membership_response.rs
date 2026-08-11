use crate::modules::identity::application::MembershipMutationResult;
use crate::modules::identity::domain::repositories::MembershipRecord;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub principal_kind: String,
    pub principal_name: String,
    pub principal_aggregate_version: u64,
    pub principal_disabled_at: Option<DateTime<Utc>>,
    pub role: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<MembershipRecord> for MembershipResponse {
    fn from(record: MembershipRecord) -> Self {
        Self {
            id: record.membership.id.as_uuid(),
            organization_id: record.membership.organization_id.as_uuid(),
            principal_id: record.principal.id.as_uuid(),
            principal_kind: record.principal.kind.as_str().to_owned(),
            principal_name: record.principal.name.as_str().to_owned(),
            principal_aggregate_version: record.principal.aggregate_version,
            principal_disabled_at: record.principal.disabled_at,
            role: record.membership.role.as_str().to_owned(),
            aggregate_version: record.membership.aggregate_version,
            created_at: record.membership.created_at,
            updated_at: record.membership.updated_at,
            revoked_at: record.membership.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipMutationResponse {
    #[serde(flatten)]
    pub membership: MembershipResponse,
    pub replayed: bool,
}

impl From<MembershipMutationResult> for MembershipMutationResponse {
    fn from(result: MembershipMutationResult) -> Self {
        Self {
            membership: result.membership.into(),
            replayed: result.replayed,
        }
    }
}
