use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateServiceMembershipRequest {
    pub name: String,
    pub role: String,
}
