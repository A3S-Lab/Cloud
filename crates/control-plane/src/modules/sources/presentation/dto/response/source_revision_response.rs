use crate::modules::sources::application::commands::accept_external_source_revision::AcceptExternalSourceRevisionResult;
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
pub struct GitRepositoryResponse {
    pub provider: String,
    pub canonical_url: String,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRecipeResponse {
    pub schema: String,
    pub kind: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub target: Option<String>,
    pub platforms: Vec<String>,
}

impl SourceRevisionResponse {
    pub fn from_result(result: AcceptExternalSourceRevisionResult) -> Self {
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
            repository: GitRepositoryResponse {
                provider: revision.repository.provider().as_str().into(),
                canonical_url: revision.repository.canonical_url().into(),
                identity: revision.repository.identity().into(),
            },
            commit_sha: revision.commit_sha.as_str().into(),
            recipe: BuildRecipeResponse {
                schema: revision.recipe.schema().into(),
                kind: revision.recipe.kind().into(),
                context_path: revision.recipe.context_path().into(),
                dockerfile_path: revision.recipe.dockerfile_path().into(),
                target: revision.recipe.target().map(str::to_owned),
                platforms: revision
                    .recipe
                    .platforms()
                    .iter()
                    .map(|platform| platform.as_str().to_owned())
                    .collect(),
            },
            recipe_digest: revision.recipe_digest,
            aggregate_version: revision.aggregate_version,
            accepted_at: revision.accepted_at,
            replayed,
        }
    }
}
