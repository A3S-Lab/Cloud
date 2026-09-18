use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceDirectoryMembershipProjectionRequest {
    pub subject_ref: String,
    pub principal_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListDirectoryMembershipProjectionsQuery {
    pub subject_ref: Option<String>,
    pub principal_id: Option<Uuid>,
}
