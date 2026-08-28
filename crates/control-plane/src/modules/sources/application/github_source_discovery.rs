use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GitCommitSha, OrganizationId, SourceConnectionId,
};
use crate::modules::sources::domain::{
    GitProvider, GitReference, GitRepository, GithubConnection, GithubInstallationId,
    IGithubConnectionRepository, SourceRepositoryPolicy,
};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE: usize = 50;
pub const MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE: usize = 100;
pub const MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES: usize = 128;
pub const GITHUB_SOURCE_DISCOVERY_CURSOR_PATTERN: &str = r"^[A-Za-z0-9_-]+$";
const MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubDiscoveredReferenceKind {
    Branch,
    Tag,
}

impl GithubDiscoveredReferenceKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "branch" => Ok(Self::Branch),
            "tag" => Ok(Self::Tag),
            _ => Err("GitHub discovered reference kind must be branch or tag".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Tag => "tag",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubDiscoveredRepository {
    pub repository: GitRepository,
    pub default_branch: String,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub disabled: bool,
}

impl GithubDiscoveredRepository {
    pub fn validate(&self) -> Result<(), String> {
        if self.repository.provider() != GitProvider::Github
            || GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?
                != self.repository
        {
            return Err("GitHub discovered repository identity is invalid".into());
        }
        GitReference::parse("branch", self.default_branch.clone())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubDiscoveredReference {
    pub kind: GithubDiscoveredReferenceKind,
    pub name: String,
    pub commit_sha: GitCommitSha,
    pub protected: Option<bool>,
}

impl GithubDiscoveredReference {
    pub fn validate(&self) -> Result<(), String> {
        GitReference::parse(self.kind.as_str(), self.name.clone())?;
        match (self.kind, self.protected) {
            (GithubDiscoveredReferenceKind::Branch, Some(_))
            | (GithubDiscoveredReferenceKind::Tag, None) => Ok(()),
            _ => Err("GitHub discovered reference protection state is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepositoryDiscoveryPage {
    pub repositories: Vec<GithubDiscoveredRepository>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepositoryReferenceDiscoveryPage {
    pub repository: GitRepository,
    pub kind: GithubDiscoveredReferenceKind,
    pub references: Vec<GithubDiscoveredReference>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubSourceDiscoveryScope {
    pub organization_id: OrganizationId,
    pub connection_id: SourceConnectionId,
    pub installation_id: GithubInstallationId,
    pub requested_at: DateTime<Utc>,
}

impl GithubSourceDiscoveryScope {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.connection_id.as_uuid().is_nil()
            || self.installation_id.as_u64() == 0
        {
            return Err("GitHub source discovery scope is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubRepositoryDiscoveryProviderRequest {
    pub scope: GithubSourceDiscoveryScope,
    pub page: u32,
    pub limit: usize,
}

impl GithubRepositoryDiscoveryProviderRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        validate_provider_page(self.page, self.limit)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepositoryReferenceDiscoveryProviderRequest {
    pub scope: GithubSourceDiscoveryScope,
    pub repository: GitRepository,
    pub kind: GithubDiscoveredReferenceKind,
    pub page: u32,
    pub limit: usize,
}

impl GithubRepositoryReferenceDiscoveryProviderRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        if self.repository.provider() != GitProvider::Github
            || GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?
                != self.repository
        {
            return Err("GitHub source discovery repository is invalid".into());
        }
        validate_provider_page(self.page, self.limit)
    }
}

fn validate_provider_page(page: u32, limit: usize) -> Result<(), String> {
    if page == 0
        || page > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER
        || limit == 0
        || limit > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE
    {
        return Err(format!(
            "GitHub source discovery page must be between 1 and {MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER} and contain at most {MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE} entries"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubSourceDiscoveryProviderPage<T> {
    pub entries: Vec<T>,
    pub has_next: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubSourceDiscoveryProviderError {
    #[error("GitHub source discovery is not configured")]
    NotConfigured,
    #[error("GitHub source discovery is forbidden")]
    Forbidden,
    #[error("GitHub source discovery provider is unavailable")]
    Unavailable,
    #[error("GitHub source discovery provider violated the protocol: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IGithubSourceDiscoveryProvider: Send + Sync {
    async fn list_repositories(
        &self,
        request: GithubRepositoryDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
        GithubSourceDiscoveryProviderError,
    >;

    async fn list_references(
        &self,
        request: GithubRepositoryReferenceDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
        GithubSourceDiscoveryProviderError,
    >;
}

pub struct GithubSourceDiscoveryQueryService {
    connections: Arc<dyn IGithubConnectionRepository>,
    provider: Arc<dyn IGithubSourceDiscoveryProvider>,
    policy: Arc<SourceRepositoryPolicy>,
}

impl GithubSourceDiscoveryQueryService {
    pub fn new(
        connections: Arc<dyn IGithubConnectionRepository>,
        provider: Arc<dyn IGithubSourceDiscoveryProvider>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self {
            connections,
            provider,
            policy,
        }
    }

    pub async fn list_repositories(
        &self,
        organization_id: OrganizationId,
        cursor: Option<&str>,
        limit: usize,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<GithubRepositoryDiscoveryPage> {
        validate_public_limit(limit)?;
        let connection = self.authoritative_connection(organization_id).await?;
        let scope = discovery_scope(&connection, requested_at);
        let cursor_scope = repository_cursor_scope(&connection);
        let page = decode_cursor(cursor, limit, &cursor_scope)?;
        let provider_page = self
            .provider
            .list_repositories(GithubRepositoryDiscoveryProviderRequest { scope, page, limit })
            .await
            .map_err(map_provider_error)?;
        if provider_page.entries.len() > limit {
            return Err(ApplicationError::Internal(
                "GitHub repository discovery provider exceeded the requested page bound".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut repositories = Vec::with_capacity(provider_page.entries.len());
        for repository in provider_page.entries {
            repository.validate().map_err(|_| {
                ApplicationError::Internal(
                    "GitHub repository discovery provider returned invalid repository state".into(),
                )
            })?;
            if !seen.insert(repository.repository.identity().to_owned()) {
                return Err(ApplicationError::Internal(
                    "GitHub repository discovery provider returned duplicate repository state"
                        .into(),
                ));
            }
            if self.policy.require(&repository.repository).is_ok() {
                repositories.push(repository);
            }
        }
        let next_cursor = provider_page
            .has_next
            .then(|| encode_next_cursor(page, limit, &cursor_scope))
            .transpose()?;
        Ok(GithubRepositoryDiscoveryPage {
            repositories,
            next_cursor,
        })
    }

    pub async fn list_references(
        &self,
        organization_id: OrganizationId,
        repository_url: &str,
        kind: &str,
        cursor: Option<&str>,
        limit: usize,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<GithubRepositoryReferenceDiscoveryPage> {
        let repository = GitRepository::parse(GitProvider::Github, repository_url)
            .map_err(ApplicationError::Invalid)?;
        if repository.canonical_url() != repository_url {
            return Err(ApplicationError::Invalid(
                "GitHub source discovery repository URL must be canonical".into(),
            ));
        }
        let kind = GithubDiscoveredReferenceKind::parse(kind).map_err(ApplicationError::Invalid)?;
        validate_public_limit(limit)?;
        let connection = self.authoritative_connection(organization_id).await?;
        self.policy
            .require(&repository)
            .map_err(ApplicationError::Forbidden)?;
        let scope = discovery_scope(&connection, requested_at);
        let cursor_scope = reference_cursor_scope(&connection, &repository, kind);
        let page = decode_cursor(cursor, limit, &cursor_scope)?;
        let provider_page = self
            .provider
            .list_references(GithubRepositoryReferenceDiscoveryProviderRequest {
                scope,
                repository: repository.clone(),
                kind,
                page,
                limit,
            })
            .await
            .map_err(map_provider_error)?;
        if provider_page.entries.len() > limit {
            return Err(ApplicationError::Internal(
                "GitHub reference discovery provider exceeded the requested page bound".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        for reference in &provider_page.entries {
            reference.validate().map_err(|_| {
                ApplicationError::Internal(
                    "GitHub reference discovery provider returned invalid reference state".into(),
                )
            })?;
            if reference.kind != kind || !seen.insert(reference.name.clone()) {
                return Err(ApplicationError::Internal(
                    "GitHub reference discovery provider returned conflicting reference state"
                        .into(),
                ));
            }
        }
        let next_cursor = provider_page
            .has_next
            .then(|| encode_next_cursor(page, limit, &cursor_scope))
            .transpose()?;
        Ok(GithubRepositoryReferenceDiscoveryPage {
            repository,
            kind,
            references: provider_page.entries,
            next_cursor,
        })
    }

    async fn authoritative_connection(
        &self,
        organization_id: OrganizationId,
    ) -> ApplicationResult<GithubConnection> {
        if organization_id.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "GitHub source discovery organization is invalid".into(),
            ));
        }
        let connection = self
            .connections
            .find(organization_id)
            .await
            .map_err(|_| {
                ApplicationError::Internal("GitHub source connection lookup failed".into())
            })?
            .ok_or_else(|| {
                ApplicationError::NotFound("GitHub source connection not found".into())
            })?;
        let connection = GithubConnection::restore(connection).map_err(|_| {
            ApplicationError::Internal(
                "stored GitHub source connection failed integrity validation".into(),
            )
        })?;
        if connection.organization_id != organization_id || !connection.is_authoritative() {
            return Err(ApplicationError::NotFound(
                "GitHub source connection not found".into(),
            ));
        }
        Ok(connection)
    }
}

fn discovery_scope(
    connection: &GithubConnection,
    requested_at: DateTime<Utc>,
) -> GithubSourceDiscoveryScope {
    GithubSourceDiscoveryScope {
        organization_id: connection.organization_id,
        connection_id: connection.id,
        installation_id: connection.installation_id,
        requested_at: canonical_timestamp(requested_at),
    }
}

fn validate_public_limit(limit: usize) -> ApplicationResult<()> {
    if limit == 0 || limit > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE {
        return Err(ApplicationError::Invalid(format!(
            "GitHub source discovery limit must be between 1 and {MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE}"
        )));
    }
    Ok(())
}

fn repository_cursor_scope(connection: &GithubConnection) -> String {
    cursor_scope_digest(&format!(
        "repositories\n{}\n{}\n{}",
        connection.organization_id,
        connection.id,
        connection.installation_id.as_u64()
    ))
}

fn reference_cursor_scope(
    connection: &GithubConnection,
    repository: &GitRepository,
    kind: GithubDiscoveredReferenceKind,
) -> String {
    cursor_scope_digest(&format!(
        "references\n{}\n{}\n{}\n{}\n{}",
        connection.organization_id,
        connection.id,
        connection.installation_id.as_u64(),
        repository.identity(),
        kind.as_str()
    ))
}

fn cursor_scope_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn encode_next_cursor(page: u32, limit: usize, scope: &str) -> ApplicationResult<String> {
    let next_page = page.checked_add(1).ok_or_else(|| {
        ApplicationError::Invalid("GitHub source discovery cursor overflowed".into())
    })?;
    if next_page > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER {
        return Err(ApplicationError::Internal(
            "GitHub source discovery provider exceeded the pagination bound".into(),
        ));
    }
    let payload = format!("v1:{next_page}:{limit}:{scope}");
    let cursor = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    if cursor.len() > MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES {
        return Err(ApplicationError::Internal(
            "GitHub source discovery cursor exceeded its bound".into(),
        ));
    }
    Ok(cursor)
}

fn decode_cursor(cursor: Option<&str>, limit: usize, scope: &str) -> ApplicationResult<u32> {
    let Some(cursor) = cursor else {
        return Ok(1);
    };
    if cursor.is_empty() || cursor.len() > MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES {
        return Err(ApplicationError::Invalid(
            "GitHub source discovery cursor is invalid".into(),
        ));
    }
    let decoded = URL_SAFE_NO_PAD.decode(cursor.as_bytes()).map_err(|_| {
        ApplicationError::Invalid("GitHub source discovery cursor is invalid".into())
    })?;
    let decoded = std::str::from_utf8(&decoded).map_err(|_| {
        ApplicationError::Invalid("GitHub source discovery cursor is invalid".into())
    })?;
    let mut parts = decoded.split(':');
    let version = parts.next();
    let page = parts.next().and_then(|value| value.parse::<u32>().ok());
    let cursor_limit = parts.next().and_then(|value| value.parse::<usize>().ok());
    let cursor_scope = parts.next();
    if version != Some("v1")
        || page.is_none_or(|page| page < 2)
        || page.is_some_and(|page| page > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER)
        || cursor_limit != Some(limit)
        || cursor_scope != Some(scope)
        || parts.next().is_some()
    {
        return Err(ApplicationError::Invalid(
            "GitHub source discovery cursor is invalid".into(),
        ));
    }
    page.ok_or_else(|| {
        ApplicationError::Invalid("GitHub source discovery cursor is invalid".into())
    })
}

fn map_provider_error(error: GithubSourceDiscoveryProviderError) -> ApplicationError {
    match error {
        GithubSourceDiscoveryProviderError::Forbidden => {
            ApplicationError::NotFound("GitHub source connection or repository not found".into())
        }
        GithubSourceDiscoveryProviderError::NotConfigured
        | GithubSourceDiscoveryProviderError::Unavailable => ApplicationError::Unavailable(
            "GitHub source discovery is temporarily unavailable".into(),
        ),
        GithubSourceDiscoveryProviderError::Protocol(message) => {
            ApplicationError::Internal(message)
        }
    }
}

#[cfg(test)]
#[path = "github_source_discovery_tests.rs"]
mod tests;
