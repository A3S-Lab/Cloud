use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProjectAttributionRequest {
    pub business_owner_reference: String,
    #[serde(default)]
    pub cost_attribution_code: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}
