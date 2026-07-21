use crate::modules::sources::application::commands::resolve_external_source_revision::ResolveExternalSourceRevisionResult;
use crate::modules::sources::domain::ExternalSourceRevision;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub repository: GitRepositoryResponse,
    pub commit_sha: String,
    pub recipe: BuildRecipeResponse,
    pub recipe_digest: String,
    pub aggregate_version: u64,
    pub accepted_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitRepositoryResponse {
    pub provider: String,
    pub canonical_url: String,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildRecipeResponse {
    pub schema: String,
    pub kind: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub target: Option<String>,
    pub platforms: Vec<String>,
}

impl SourceRevisionResponse {
    pub fn from_result(result: ResolveExternalSourceRevisionResult) -> Self {
        Self::new(result.revision, Some(result.replayed))
    }

    pub fn from_revision(revision: ExternalSourceRevision) -> Self {
        Self::new(revision, None)
    }

    fn new(revision: ExternalSourceRevision, replayed: Option<bool>) -> Self {
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            environment_id: revision.environment_id.as_uuid(),
            id: revision.id.as_uuid(),
            repository: GitRepositoryResponse::from(&revision.repository),
            commit_sha: revision.commit_sha.as_str().into(),
            recipe: BuildRecipeResponse::from(&revision.recipe),
            recipe_digest: revision.recipe_digest,
            aggregate_version: revision.aggregate_version,
            accepted_at: revision.accepted_at,
            replayed,
        }
    }
}

impl From<&crate::modules::sources::domain::GitRepository> for GitRepositoryResponse {
    fn from(repository: &crate::modules::sources::domain::GitRepository) -> Self {
        Self {
            provider: repository.provider().as_str().into(),
            canonical_url: repository.canonical_url().into(),
            identity: repository.identity().into(),
        }
    }
}

impl From<&crate::modules::sources::domain::BuildRecipe> for BuildRecipeResponse {
    fn from(recipe: &crate::modules::sources::domain::BuildRecipe) -> Self {
        Self {
            schema: recipe.schema().into(),
            kind: recipe.kind().into(),
            context_path: recipe.context_path().into(),
            dockerfile_path: recipe.dockerfile_path().into(),
            target: recipe.target().map(str::to_owned),
            platforms: recipe
                .platforms()
                .iter()
                .map(|platform| platform.as_str().to_owned())
                .collect(),
        }
    }
}
