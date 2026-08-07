use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMembershipRoleRequest {
    pub role: String,
    pub expected_version: u64,
}
