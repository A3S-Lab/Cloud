use crate::modules::identity::presentation::dto::ResourceGrantScopeDto;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDirectoryResourceGrantRequest {
    pub subject_ref: String,
    pub scope: ResourceGrantScopeDto,
}
