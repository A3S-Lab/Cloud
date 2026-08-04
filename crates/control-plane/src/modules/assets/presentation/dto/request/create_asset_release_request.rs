use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAssetReleaseRequest {
    pub version: String,
    pub commit_sha: String,
}
