use crate::modules::identity::application::ResourceGrantMutationResult;
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::presentation::dto::ResourceGrantScopeDto;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub membership_id: Uuid,
    pub scope: ResourceGrantScopeDto,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<ResourceGrant> for ResourceGrantResponse {
    fn from(grant: ResourceGrant) -> Self {
        Self {
            id: grant.id.as_uuid(),
            organization_id: grant.organization_id.as_uuid(),
            membership_id: grant.membership_id.as_uuid(),
            scope: grant.scope.into(),
            aggregate_version: grant.aggregate_version,
            created_at: grant.created_at,
            updated_at: grant.updated_at,
            revoked_at: grant.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantMutationResponse {
    #[serde(flatten)]
    pub resource_grant: ResourceGrantResponse,
    pub replayed: bool,
}

impl From<ResourceGrantMutationResult> for ResourceGrantMutationResponse {
    fn from(result: ResourceGrantMutationResult) -> Self {
        Self {
            resource_grant: result.resource_grant.into(),
            replayed: result.replayed,
        }
    }
}
