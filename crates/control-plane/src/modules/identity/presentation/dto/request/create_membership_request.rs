use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMembershipRequest {
    #[serde(default = "default_principal_kind")]
    pub principal_kind: String,
    pub name: String,
    pub role: String,
}

fn default_principal_kind() -> String {
    "service".into()
}
