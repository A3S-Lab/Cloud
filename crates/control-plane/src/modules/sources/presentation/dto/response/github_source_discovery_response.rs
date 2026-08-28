use super::GitRepositoryResponse;
use crate::modules::sources::{
    GithubDiscoveredReference, GithubDiscoveredRepository, GithubRepositoryDiscoveryPage,
    GithubRepositoryReferenceDiscoveryPage,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepositoryDiscoveryPageResponse {
    pub repositories: Vec<GithubDiscoveredRepositoryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDiscoveredRepositoryResponse {
    pub repository: GitRepositoryResponse,
    pub default_branch: String,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepositoryReferenceDiscoveryPageResponse {
    pub repository: GitRepositoryResponse,
    pub kind: String,
    pub references: Vec<GithubDiscoveredReferenceResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDiscoveredReferenceResponse {
    pub kind: String,
    pub name: String,
    pub commit_sha: String,
    pub protected: Option<bool>,
}

impl From<GithubRepositoryDiscoveryPage> for GithubRepositoryDiscoveryPageResponse {
    fn from(page: GithubRepositoryDiscoveryPage) -> Self {
        Self {
            repositories: page
                .repositories
                .into_iter()
                .map(GithubDiscoveredRepositoryResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

impl From<GithubDiscoveredRepository> for GithubDiscoveredRepositoryResponse {
    fn from(repository: GithubDiscoveredRepository) -> Self {
        Self {
            repository: GitRepositoryResponse::from(&repository.repository),
            default_branch: repository.default_branch,
            private: repository.private,
            fork: repository.fork,
            archived: repository.archived,
            disabled: repository.disabled,
        }
    }
}

impl From<GithubRepositoryReferenceDiscoveryPage>
    for GithubRepositoryReferenceDiscoveryPageResponse
{
    fn from(page: GithubRepositoryReferenceDiscoveryPage) -> Self {
        Self {
            repository: GitRepositoryResponse::from(&page.repository),
            kind: page.kind.as_str().into(),
            references: page
                .references
                .into_iter()
                .map(GithubDiscoveredReferenceResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

impl From<GithubDiscoveredReference> for GithubDiscoveredReferenceResponse {
    fn from(reference: GithubDiscoveredReference) -> Self {
        Self {
            kind: reference.kind.as_str().into(),
            name: reference.name,
            commit_sha: reference.commit_sha.as_str().into(),
            protected: reference.protected,
        }
    }
}
