use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptSourceRevisionRequest {
    pub repository: GitRepositoryRequest,
    pub commit_sha: String,
    pub recipe: DockerfileBuildRecipeRequest,
    pub webhook_delivery_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitRepositoryRequest {
    pub provider: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DockerfileBuildRecipeRequest {
    pub schema: String,
    pub kind: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub target: Option<String>,
    pub platforms: Vec<String>,
}
