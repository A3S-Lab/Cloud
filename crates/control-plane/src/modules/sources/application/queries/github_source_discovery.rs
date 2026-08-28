use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::application::{
    GithubRepositoryDiscoveryPage, GithubRepositoryReferenceDiscoveryPage,
    GithubSourceDiscoveryQueryService,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGithubInstallationRepositories {
    pub organization_id: OrganizationId,
    pub cursor: Option<String>,
    pub limit: usize,
    pub requested_at: DateTime<Utc>,
}

impl Query for ListGithubInstallationRepositories {
    type Output = ApplicationResult<GithubRepositoryDiscoveryPage>;
}

pub struct ListGithubInstallationRepositoriesHandler {
    discovery: Arc<GithubSourceDiscoveryQueryService>,
}

impl ListGithubInstallationRepositoriesHandler {
    pub fn new(discovery: Arc<GithubSourceDiscoveryQueryService>) -> Self {
        Self { discovery }
    }
}

impl QueryHandler<ListGithubInstallationRepositories>
    for ListGithubInstallationRepositoriesHandler
{
    fn execute(
        &self,
        query: ListGithubInstallationRepositories,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GithubRepositoryDiscoveryPage>>,
    > {
        let discovery = Arc::clone(&self.discovery);
        Box::pin(async move {
            Ok(discovery
                .list_repositories(
                    query.organization_id,
                    query.cursor.as_deref(),
                    query.limit,
                    query.requested_at,
                )
                .await)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGithubRepositoryReferences {
    pub organization_id: OrganizationId,
    pub repository_url: String,
    pub kind: String,
    pub cursor: Option<String>,
    pub limit: usize,
    pub requested_at: DateTime<Utc>,
}

impl Query for ListGithubRepositoryReferences {
    type Output = ApplicationResult<GithubRepositoryReferenceDiscoveryPage>;
}

pub struct ListGithubRepositoryReferencesHandler {
    discovery: Arc<GithubSourceDiscoveryQueryService>,
}

impl ListGithubRepositoryReferencesHandler {
    pub fn new(discovery: Arc<GithubSourceDiscoveryQueryService>) -> Self {
        Self { discovery }
    }
}

impl QueryHandler<ListGithubRepositoryReferences> for ListGithubRepositoryReferencesHandler {
    fn execute(
        &self,
        query: ListGithubRepositoryReferences,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GithubRepositoryReferenceDiscoveryPage>>,
    > {
        let discovery = Arc::clone(&self.discovery);
        Box::pin(async move {
            Ok(discovery
                .list_references(
                    query.organization_id,
                    &query.repository_url,
                    &query.kind,
                    query.cursor.as_deref(),
                    query.limit,
                    query.requested_at,
                )
                .await)
        })
    }
}
