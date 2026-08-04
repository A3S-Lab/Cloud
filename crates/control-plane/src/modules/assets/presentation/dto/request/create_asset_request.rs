use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAssetRequest {
    pub name: String,
    pub kind: String,
}
