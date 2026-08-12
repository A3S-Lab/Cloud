use crate::modules::identity::presentation::dto::ResourceGrantScopeDto;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateResourceGrantRequest {
    pub scope: ResourceGrantScopeDto,
}
