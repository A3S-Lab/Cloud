use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, ISourceResolver, ResolvedSource,
    SourceProviderCredential, SourceResolutionError, SourceResolutionRequest,
};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::time::Duration;
use url::Url;

const GITHUB_API_URL: &str = "https://api.github.com/";
const GITHUB_API_VERSION: &str = "2022-11-28";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
const MAX_TAG_DEPTH: usize = 8;

#[derive(Clone)]
pub struct GithubSourceResolver {
    client: Client,
    api_base: Url,
    request_timeout: Duration,
}

impl GithubSourceResolver {
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let api_base = Url::parse(GITHUB_API_URL)
            .map_err(|error| format!("GitHub API URL is invalid: {error}"))?;
        Self::with_api_base(timeout, api_base, false)
    }

    fn with_api_base(timeout: Duration, api_base: Url, allow_http: bool) -> Result<Self, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err("GitHub request timeout must be between 1 ms and 60 seconds".into());
        }
        if !matches!(api_base.path(), "" | "/")
            || api_base.username() != ""
            || api_base.password().is_some()
            || api_base.query().is_some()
            || api_base.fragment().is_some()
            || api_base.host_str().is_none()
            || !(api_base.scheme() == "https" || allow_http && api_base.scheme() == "http")
        {
            return Err("GitHub API endpoint is invalid".into());
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static(GITHUB_API_VERSION),
        );
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!allow_http)
            .user_agent("a3s-cloud-control-plane")
            .default_headers(headers)
            .build()
            .map_err(|error| format!("could not build GitHub API client: {error}"))?;
        Ok(Self {
            client,
            api_base,
            request_timeout: timeout,
        })
    }

    #[cfg(test)]
    fn for_test(timeout: Duration, api_base: Url) -> Result<Self, String> {
        Self::with_api_base(timeout, api_base, true)
    }

    async fn confirm_repository(
        &self,
        repository: &GitRepository,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<(), SourceResolutionError> {
        let (owner, name) = repository_coordinates(repository)?;
        let response: GithubRepositoryResponse = self
            .get_json(self.api_url(&["repos", owner, name])?, credential)
            .await?;
        let expected_name = format!("{owner}/{name}");
        let response_repository = GitRepository::parse(GitProvider::Github, &response.html_url)
            .map_err(|_| SourceResolutionError::Unavailable)?;
        if response.private && credential.is_none()
            || !response.full_name.eq_ignore_ascii_case(&expected_name)
            || response_repository != *repository
        {
            return Err(SourceResolutionError::Unavailable);
        }
        Ok(())
    }

    async fn resolve_reference(
        &self,
        repository: &GitRepository,
        reference: &GitReference,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<GitCommitSha, SourceResolutionError> {
        let (owner, name) = repository_coordinates(repository)?;
        match reference {
            GitReference::Commit(commit_sha) => {
                let response: GithubCommitResponse = self
                    .get_json(
                        self.api_url(&[
                            "repos",
                            owner,
                            name,
                            "git",
                            "commits",
                            commit_sha.as_str(),
                        ])?,
                        credential,
                    )
                    .await?;
                let resolved = parse_sha(response.sha)?;
                if &resolved != commit_sha {
                    return Err(protocol("GitHub returned a different commit object ID"));
                }
                Ok(resolved)
            }
            GitReference::Branch(branch) => {
                let response = self
                    .git_reference(owner, name, "heads", branch, credential)
                    .await?;
                if response.object.kind != "commit" {
                    return Err(protocol("GitHub branch did not resolve to a commit"));
                }
                parse_sha(response.object.sha)
            }
            GitReference::Tag(tag) => {
                let response = self
                    .git_reference(owner, name, "tags", tag, credential)
                    .await?;
                self.peel_tag(owner, name, response.object, credential)
                    .await
            }
        }
    }

    async fn git_reference(
        &self,
        owner: &str,
        repository: &str,
        namespace: &str,
        name: &str,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<GithubReferenceResponse, SourceResolutionError> {
        let mut segments = vec!["repos", owner, repository, "git", "ref", namespace];
        segments.extend(name.split('/'));
        let response: GithubReferenceResponse =
            self.get_json(self.api_url(&segments)?, credential).await?;
        let expected = format!("refs/{namespace}/{name}");
        if response.reference != expected {
            return Err(protocol("GitHub returned a different Git reference"));
        }
        Ok(response)
    }

    async fn peel_tag(
        &self,
        owner: &str,
        repository: &str,
        mut object: GithubObject,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<GitCommitSha, SourceResolutionError> {
        let mut visited = BTreeSet::new();
        for _ in 0..MAX_TAG_DEPTH {
            match object.kind.as_str() {
                "commit" => return parse_sha(object.sha),
                "tag" => {
                    let tag_sha = parse_sha(object.sha)?;
                    if !visited.insert(tag_sha.clone()) {
                        return Err(protocol("GitHub annotated tag chain contains a cycle"));
                    }
                    let response: GithubTagResponse = self
                        .get_json(
                            self.api_url(&[
                                "repos",
                                owner,
                                repository,
                                "git",
                                "tags",
                                tag_sha.as_str(),
                            ])?,
                            credential,
                        )
                        .await?;
                    if parse_sha(response.sha)? != tag_sha {
                        return Err(protocol("GitHub returned a different annotated tag"));
                    }
                    object = response.object;
                }
                _ => return Err(protocol("GitHub tag did not resolve to a commit")),
            }
        }
        Err(protocol("GitHub annotated tag chain is too deep"))
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url, SourceResolutionError> {
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(segments);
        Ok(url)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        url: Url,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<T, SourceResolutionError> {
        let request = self.client.get(url);
        let request = match credential {
            Some(credential) => request.bearer_auth(credential.expose_token()),
            None => request,
        };
        let mut response = request.send().await.map_err(|_| {
            SourceResolutionError::ProviderUnavailable("GitHub request failed".into())
        })?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                return Err(SourceResolutionError::Unavailable)
            }
            status if status.is_redirection() => return Err(SourceResolutionError::Unavailable),
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(SourceResolutionError::ProviderUnavailable(format!(
                    "GitHub returned HTTP {status}"
                )))
            }
            status => {
                return Err(protocol(format!(
                    "GitHub returned unexpected HTTP {status}"
                )))
            }
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES)
        {
            return Err(protocol("GitHub response exceeded the size limit"));
        }
        let mut body = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or(0)
                .min(MAX_RESPONSE_BYTES) as usize,
        );
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            SourceResolutionError::ProviderUnavailable(
                "GitHub response body could not be read".into(),
            )
        })? {
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|length| length as u64 > MAX_RESPONSE_BYTES)
            {
                return Err(protocol("GitHub response exceeded the size limit"));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|error| protocol(format!("GitHub response JSON is invalid: {error}")))
    }
}

#[async_trait]
impl ISourceResolver for GithubSourceResolver {
    async fn resolve(
        &self,
        request: &SourceResolutionRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<ResolvedSource, SourceResolutionError> {
        if request.repository.provider() != GitProvider::Github {
            return Err(SourceResolutionError::Unavailable);
        }
        if credential.is_some_and(|credential| {
            !credential.authorizes(
                &request.repository,
                chrono::Utc::now(),
                self.minimum_credential_lifetime(&request.reference),
            )
        }) {
            return Err(SourceResolutionError::Unavailable);
        }
        self.confirm_repository(&request.repository, credential)
            .await?;
        let commit_sha = self
            .resolve_reference(&request.repository, &request.reference, credential)
            .await?;
        Ok(ResolvedSource {
            repository: request.repository.clone(),
            commit_sha,
        })
    }
}

impl GithubSourceResolver {
    fn minimum_credential_lifetime(&self, reference: &GitReference) -> chrono::Duration {
        let maximum_requests = if matches!(reference, GitReference::Tag(_)) {
            MAX_TAG_DEPTH as u32 + 2
        } else {
            2
        };
        chrono::Duration::from_std(
            self.request_timeout
                .checked_mul(maximum_requests)
                .unwrap_or(Duration::from_secs(600)),
        )
        .unwrap_or_else(|_| chrono::Duration::minutes(10))
    }
}

#[derive(Deserialize)]
struct GithubRepositoryResponse {
    full_name: String,
    html_url: String,
    private: bool,
}

#[derive(Deserialize)]
struct GithubReferenceResponse {
    #[serde(rename = "ref")]
    reference: String,
    object: GithubObject,
}

#[derive(Deserialize)]
struct GithubObject {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

#[derive(Deserialize)]
struct GithubTagResponse {
    sha: String,
    object: GithubObject,
}

#[derive(Deserialize)]
struct GithubCommitResponse {
    sha: String,
}

fn repository_coordinates(
    repository: &GitRepository,
) -> Result<(&str, &str), SourceResolutionError> {
    repository
        .owner_and_name()
        .ok_or_else(|| protocol("canonical GitHub repository coordinates are unavailable"))
}

fn parse_sha(value: String) -> Result<GitCommitSha, SourceResolutionError> {
    GitCommitSha::parse(value).map_err(|_| protocol("GitHub returned an invalid Git object ID"))
}

fn protocol(message: impl Into<String>) -> SourceResolutionError {
    SourceResolutionError::Protocol(message.into())
}

#[cfg(test)]
#[path = "github_source_resolver_tests.rs"]
mod tests;
