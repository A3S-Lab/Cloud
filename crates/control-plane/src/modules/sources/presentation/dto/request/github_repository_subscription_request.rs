use super::{DockerfileBuildRecipeRequest, GitRepositoryRequest};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGithubRepositorySubscriptionRequest {
    pub repository: GitRepositoryRequest,
    pub branch: String,
    pub recipe: DockerfileBuildRecipeRequest,
}
