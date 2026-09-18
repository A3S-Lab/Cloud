use crate::modules::identity::application::DirectoryMembershipProjectionMutationResult;
use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryMembershipProjectionBindingResponse {
    pub organization_id: Uuid,
    pub subject_ref: String,
    pub kind: String,
    pub issuer: String,
    pub subject_id: Uuid,
    pub principal_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<DirectoryMembershipProjectionBinding> for DirectoryMembershipProjectionBindingResponse {
    fn from(binding: DirectoryMembershipProjectionBinding) -> Self {
        Self {
            organization_id: binding.organization_id.as_uuid(),
            subject_ref: binding.subject.format_ref(),
            kind: binding.subject.kind().as_str().to_owned(),
            issuer: binding.subject.issuer().as_str().to_owned(),
            subject_id: binding.subject.subject_id(),
            principal_id: binding.principal_id.as_uuid(),
            created_at: binding.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryMembershipProjectionMutationResponse {
    pub items: Vec<DirectoryMembershipProjectionBindingResponse>,
    pub replayed: bool,
}

impl From<DirectoryMembershipProjectionMutationResult>
    for DirectoryMembershipProjectionMutationResponse
{
    fn from(result: DirectoryMembershipProjectionMutationResult) -> Self {
        Self {
            items: result
                .bindings
                .into_iter()
                .map(DirectoryMembershipProjectionBindingResponse::from)
                .collect(),
            replayed: result.replayed,
        }
    }
}
