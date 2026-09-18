use crate::modules::identity::application::DirectoryResourceGrantMutationResult;
use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::presentation::dto::ResourceGrantScopeDto;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResourceGrantResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub subject_ref: String,
    pub kind: String,
    pub issuer: String,
    pub subject_id: Uuid,
    pub scope: ResourceGrantScopeDto,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<DirectoryResourceGrant> for DirectoryResourceGrantResponse {
    fn from(grant: DirectoryResourceGrant) -> Self {
        Self {
            id: grant.id.as_uuid(),
            organization_id: grant.organization_id.as_uuid(),
            subject_ref: grant.subject.format_ref(),
            kind: grant.subject.kind().as_str().to_owned(),
            issuer: grant.subject.issuer().as_str().to_owned(),
            subject_id: grant.subject.subject_id(),
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
pub struct DirectoryResourceGrantMutationResponse {
    #[serde(flatten)]
    pub directory_resource_grant: DirectoryResourceGrantResponse,
    pub replayed: bool,
}

impl From<DirectoryResourceGrantMutationResult> for DirectoryResourceGrantMutationResponse {
    fn from(result: DirectoryResourceGrantMutationResult) -> Self {
        Self {
            directory_resource_grant: result.directory_resource_grant.into(),
            replayed: result.replayed,
        }
    }
}
