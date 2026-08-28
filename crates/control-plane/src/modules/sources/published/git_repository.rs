use super::GitProvider;
use crate::modules::shared_kernel::domain::GitCommitSha;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRepository {
    provider: GitProvider,
    canonical_url: String,
    identity: String,
}

impl GitRepository {
    const GITHUB_COORDINATE_PATTERN: &'static str = r"(?:[a-z0-9]|(?![a-z0-9-]*--)[a-z0-9][a-z0-9-]{0,37}[a-z0-9])/(?!\.{1,2}$)(?![a-z0-9._-]*\.git$)[a-z0-9._-]{1,100}";
    pub const MAX_GITHUB_OWNER_BYTES: usize = 39;
    pub const MAX_GITHUB_REPOSITORY_NAME_BYTES: usize = 100;
    pub const MAX_CANONICAL_URL_BYTES: usize = "https://github.com/".len()
        + Self::MAX_GITHUB_OWNER_BYTES
        + 1
        + Self::MAX_GITHUB_REPOSITORY_NAME_BYTES;
    pub const MAX_IDENTITY_BYTES: usize = "github:github.com/".len()
        + Self::MAX_GITHUB_OWNER_BYTES
        + 1
        + Self::MAX_GITHUB_REPOSITORY_NAME_BYTES;

    pub fn github_canonical_url_pattern() -> String {
        format!("^https://github\\.com/{}$", Self::GITHUB_COORDINATE_PATTERN)
    }

    pub fn github_identity_pattern() -> String {
        format!("^github:github\\.com/{}$", Self::GITHUB_COORDINATE_PATTERN)
    }

    pub fn parse(provider: GitProvider, value: &str) -> Result<Self, String> {
        match provider {
            GitProvider::Github => Self::parse_github(value),
        }
    }

    fn parse_github(value: &str) -> Result<Self, String> {
        let url = Url::parse(value).map_err(|_| "Git repository URL is invalid")?;
        if url.scheme() != "https" {
            return Err("GitHub repository URL must use HTTPS".into());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("GitHub repository URL cannot contain user information".into());
        }
        if url.port().is_some() {
            return Err("GitHub repository URL cannot contain a port".into());
        }
        if url.host_str() != Some("github.com") {
            return Err("GitHub repository URL must use the exact github.com host".into());
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err("GitHub repository URL cannot contain a query or fragment".into());
        }
        let path = url.path().strip_prefix('/').unwrap_or(url.path());
        let path = path.strip_suffix('/').unwrap_or(path);
        if path.contains(['%', '\\']) {
            return Err(
                "GitHub repository URL path cannot contain encoded or escaped bytes".into(),
            );
        }
        let segments = path.split('/').collect::<Vec<_>>();
        if segments.len() != 2 || segments.iter().any(|segment| segment.is_empty()) {
            return Err(
                "GitHub repository URL must identify exactly one owner and repository".into(),
            );
        }
        let owner = segments[0].to_ascii_lowercase();
        let raw_repository = segments[1];
        let repository = if raw_repository
            .get(raw_repository.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git"))
        {
            &raw_repository[..raw_repository.len() - 4]
        } else {
            raw_repository
        }
        .to_ascii_lowercase();
        validate_github_owner(&owner)?;
        validate_github_repository(&repository)?;
        let canonical_url = format!("https://github.com/{owner}/{repository}");
        let identity = format!("github:github.com/{owner}/{repository}");
        Ok(Self {
            provider: GitProvider::Github,
            canonical_url,
            identity,
        })
    }

    pub const fn provider(&self) -> GitProvider {
        self.provider
    }

    pub fn canonical_url(&self) -> &str {
        &self.canonical_url
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn owner_and_name(&self) -> Option<(&str, &str)> {
        let path = self.canonical_url.strip_prefix("https://github.com/")?;
        path.split_once('/')
    }

    pub fn source_identity_digest(&self, commit_sha: &GitCommitSha) -> String {
        let mut digest = Sha256::new();
        digest.update(self.identity.as_bytes());
        digest.update(b"\n");
        digest.update(commit_sha.as_str().as_bytes());
        format!("sha256:{:x}", digest.finalize())
    }
}

fn validate_github_owner(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > GitRepository::MAX_GITHUB_OWNER_BYTES
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("GitHub repository owner is invalid".into());
    }
    Ok(())
}

fn validate_github_repository(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > GitRepository::MAX_GITHUB_REPOSITORY_NAME_BYTES
        || matches!(value, "." | "..")
        || value.ends_with(".git")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("GitHub repository name is invalid".into());
    }
    Ok(())
}
