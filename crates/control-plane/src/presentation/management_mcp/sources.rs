use super::tool_result;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::{
    GithubRepositoryDiscoveryPageResponse, GithubRepositoryReferenceDiscoveryPageResponse,
    ListGithubInstallationRepositories, ListGithubRepositoryReferences,
    DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
};
use a3s_boot::{QueryBus, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GithubInstallationRepositoriesArguments {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GithubRepositoryReferencesArguments {
    repository_url: String,
    kind: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

pub async fn list_installation_repositories(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: GithubInstallationRepositoriesArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListGithubInstallationRepositories {
            organization_id,
            cursor: arguments.cursor,
            limit: arguments.limit,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(page) => tool_result::success(
            200,
            GithubRepositoryDiscoveryPageResponse::from(page),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_repository_references(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: GithubRepositoryReferencesArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListGithubRepositoryReferences {
            organization_id,
            repository_url: arguments.repository_url,
            kind: arguments.kind,
            cursor: arguments.cursor,
            limit: arguments.limit,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(page) => tool_result::success(
            200,
            GithubRepositoryReferenceDiscoveryPageResponse::from(page),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

const fn default_limit() -> usize {
    DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE
}
