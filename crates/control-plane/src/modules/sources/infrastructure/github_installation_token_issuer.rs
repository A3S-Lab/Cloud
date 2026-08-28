use crate::modules::shared_kernel::domain::{canonical_timestamp, GitCommitSha};
use crate::modules::sources::application::{
    GithubDiscoveredReference, GithubDiscoveredReferenceKind, GithubDiscoveredRepository,
    GithubRepositoryDiscoveryProviderRequest, GithubRepositoryReferenceDiscoveryProviderRequest,
    GithubSourceDiscoveryProviderError, GithubSourceDiscoveryProviderPage,
    IGithubSourceDiscoveryProvider,
};
use crate::modules::sources::domain::{
    GitProvider, GitReference, GithubAccountId, GithubAccountKind, GithubInstallationAccount,
    GithubInstallationAuthorityError, GithubInstallationAuthorityRequest,
    GithubInstallationTokenError, GithubInstallationTokenRequest, GithubLogin,
    GithubProviderAuthority, IGithubInstallationAuthorityProvider, IGithubInstallationTokenService,
    SourceProviderCredential,
};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, LINK, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use rustls::pki_types::PrivateKeyDer;
use rustls_pemfile::Item;
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const GITHUB_API_URL: &str = "https://api.github.com/";
const GITHUB_API_VERSION: &str = "2022-11-28";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
const MAX_DISCOVERY_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_LINK_HEADER_BYTES: usize = 8 * 1024;
const MAX_PRIVATE_KEY_BYTES: usize = 64 * 1024;
const JWT_BACKDATE: ChronoDuration = ChronoDuration::minutes(1);
const JWT_FUTURE_LIFETIME: ChronoDuration = ChronoDuration::minutes(9);

#[derive(Clone)]
pub struct GithubInstallationTokenIssuer {
    enabled: Option<EnabledGithubInstallationTokenIssuer>,
}

#[derive(Clone)]
struct EnabledGithubInstallationTokenIssuer {
    client: Client,
    client_id: String,
    private_key_env: String,
    api_base: Url,
}

impl GithubInstallationTokenIssuer {
    pub const fn disabled() -> Self {
        Self { enabled: None }
    }

    pub fn new(
        timeout: Duration,
        client_id: impl Into<String>,
        private_key_env: impl Into<String>,
    ) -> Result<Self, String> {
        let api_base = Url::parse(GITHUB_API_URL)
            .map_err(|error| format!("GitHub API URL is invalid: {error}"))?;
        Self::with_api_base(
            timeout,
            client_id.into(),
            private_key_env.into(),
            api_base,
            false,
        )
    }

    fn with_api_base(
        timeout: Duration,
        client_id: String,
        private_key_env: String,
        api_base: Url,
        allow_http: bool,
    ) -> Result<Self, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err("GitHub request timeout must be between 1 ms and 60 seconds".into());
        }
        if !valid_client_id(&client_id)
            || !valid_env_name(&private_key_env)
            || !valid_endpoint(&api_base, allow_http)
        {
            return Err("GitHub installation-token configuration is invalid".into());
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
            .use_rustls_tls()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!allow_http)
            .user_agent("a3s-cloud-control-plane")
            .default_headers(headers)
            .build()
            .map_err(|error| {
                format!("could not build GitHub installation-token client: {error}")
            })?;
        Ok(Self {
            enabled: Some(EnabledGithubInstallationTokenIssuer {
                client,
                client_id,
                private_key_env,
                api_base,
            }),
        })
    }

    #[cfg(test)]
    fn for_test(
        timeout: Duration,
        client_id: &str,
        private_key_env: &str,
        api_base: Url,
    ) -> Result<Self, String> {
        Self::with_api_base(
            timeout,
            client_id.into(),
            private_key_env.into(),
            api_base,
            true,
        )
    }

    fn require_enabled(
        &self,
    ) -> Result<&EnabledGithubInstallationTokenIssuer, GithubInstallationTokenError> {
        self.enabled
            .as_ref()
            .ok_or(GithubInstallationTokenError::NotConfigured)
    }

    fn require_authority_enabled(
        &self,
    ) -> Result<&EnabledGithubInstallationTokenIssuer, GithubInstallationAuthorityError> {
        self.enabled
            .as_ref()
            .ok_or(GithubInstallationAuthorityError::NotConfigured)
    }

    fn require_discovery_enabled(
        &self,
    ) -> Result<&EnabledGithubInstallationTokenIssuer, GithubSourceDiscoveryProviderError> {
        self.enabled
            .as_ref()
            .ok_or(GithubSourceDiscoveryProviderError::NotConfigured)
    }
}

#[async_trait]
impl IGithubInstallationTokenService for GithubInstallationTokenIssuer {
    async fn issue(
        &self,
        request: GithubInstallationTokenRequest,
    ) -> Result<SourceProviderCredential, GithubInstallationTokenError> {
        let enabled = self.require_enabled()?;
        if request.repository.provider() != GitProvider::Github {
            return Err(protocol("installation token requires a GitHub repository"));
        }
        let (_, repository_name) = request
            .repository
            .owner_and_name()
            .ok_or_else(|| protocol("canonical GitHub repository coordinates are unavailable"))?;
        let requested_at = canonical_timestamp(request.requested_at);
        let token_response = enabled
            .issue_installation_token(
                request.installation_id.as_u64(),
                requested_at,
                Some(repository_name),
            )
            .await?;
        SourceProviderCredential::new(
            &request.repository,
            token_response.token.0,
            requested_at,
            token_response.expires_at,
        )
        .map_err(|error| protocol(format!("GitHub installation token is invalid: {error}")))
    }
}

#[async_trait]
impl IGithubSourceDiscoveryProvider for GithubInstallationTokenIssuer {
    async fn list_repositories(
        &self,
        request: GithubRepositoryDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
        GithubSourceDiscoveryProviderError,
    > {
        request
            .validate()
            .map_err(GithubSourceDiscoveryProviderError::Protocol)?;
        let enabled = self.require_discovery_enabled()?;
        let requested_at = canonical_timestamp(request.scope.requested_at);
        let token = enabled
            .issue_installation_token(request.scope.installation_id.as_u64(), requested_at, None)
            .await
            .map_err(map_discovery_token_error)?;
        let mut url = enabled.discovery_url(&["installation", "repositories"])?;
        url.query_pairs_mut()
            .append_pair("per_page", &request.limit.to_string())
            .append_pair("page", &request.page.to_string());
        let (response, has_next): (InstallationRepositoriesResponse, bool) = enabled
            .get_discovery_json(url, token.token.0.as_str())
            .await?;
        if response.repositories.len() > request.limit {
            return Err(discovery_protocol(
                "GitHub repository discovery response exceeded the requested page size",
            ));
        }
        let mut repositories = Vec::with_capacity(response.repositories.len());
        for value in response.repositories {
            if let Some(repository) = discovered_repository(value)? {
                repositories.push(repository);
            }
        }
        Ok(GithubSourceDiscoveryProviderPage {
            entries: repositories,
            has_next,
        })
    }

    async fn list_references(
        &self,
        request: GithubRepositoryReferenceDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
        GithubSourceDiscoveryProviderError,
    > {
        request
            .validate()
            .map_err(GithubSourceDiscoveryProviderError::Protocol)?;
        let enabled = self.require_discovery_enabled()?;
        let (owner, repository_name) = request.repository.owner_and_name().ok_or_else(|| {
            discovery_protocol("canonical GitHub repository coordinates are unavailable")
        })?;
        let requested_at = canonical_timestamp(request.scope.requested_at);
        let token = enabled
            .issue_installation_token(
                request.scope.installation_id.as_u64(),
                requested_at,
                Some(repository_name),
            )
            .await
            .map_err(map_discovery_token_error)?;
        let endpoint = match request.kind {
            GithubDiscoveredReferenceKind::Branch => "branches",
            GithubDiscoveredReferenceKind::Tag => "tags",
        };
        let mut url = enabled.discovery_url(&["repos", owner, repository_name, endpoint])?;
        url.query_pairs_mut()
            .append_pair("per_page", &request.limit.to_string())
            .append_pair("page", &request.page.to_string());
        let (entries, has_next) = match request.kind {
            GithubDiscoveredReferenceKind::Branch => {
                let (response, has_next): (Vec<BranchResponse>, bool) = enabled
                    .get_discovery_json(url, token.token.0.as_str())
                    .await?;
                if response.len() > request.limit {
                    return Err(discovery_protocol(
                        "GitHub branch discovery response exceeded the requested page size",
                    ));
                }
                let mut entries = Vec::with_capacity(response.len());
                for value in response {
                    if let Some(reference) = discovered_branch(value)? {
                        entries.push(reference);
                    }
                }
                (entries, has_next)
            }
            GithubDiscoveredReferenceKind::Tag => {
                let (response, has_next): (Vec<TagResponse>, bool) = enabled
                    .get_discovery_json(url, token.token.0.as_str())
                    .await?;
                if response.len() > request.limit {
                    return Err(discovery_protocol(
                        "GitHub tag discovery response exceeded the requested page size",
                    ));
                }
                let mut entries = Vec::with_capacity(response.len());
                for value in response {
                    if let Some(reference) = discovered_tag(value)? {
                        entries.push(reference);
                    }
                }
                (entries, has_next)
            }
        };
        Ok(GithubSourceDiscoveryProviderPage { entries, has_next })
    }
}

#[async_trait]
impl IGithubInstallationAuthorityProvider for GithubInstallationTokenIssuer {
    async fn inspect(
        &self,
        request: GithubInstallationAuthorityRequest,
    ) -> Result<GithubProviderAuthority, GithubInstallationAuthorityError> {
        let enabled = self.require_authority_enabled()?;
        let checked_at = canonical_timestamp(request.checked_at);
        let jwt = enabled
            .app_jwt(checked_at)
            .map_err(map_authority_signing_error)?;
        let installation_id = request.installation_id.as_u64().to_string();
        let mut url = enabled.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| authority_protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(["app", "installations", &installation_id]);
        let mut response = enabled
            .client
            .get(url)
            .bearer_auth(jwt.as_str())
            .send()
            .await
            .map_err(|_| GithubInstallationAuthorityError::Unavailable)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => {
                return Ok(GithubProviderAuthority::deleted(request.installation_id))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
                return Err(GithubInstallationAuthorityError::Unavailable)
            }
            status if status.is_server_error() => {
                return Err(GithubInstallationAuthorityError::Unavailable)
            }
            status => {
                return Err(authority_protocol(format!(
                    "GitHub installation endpoint returned unexpected HTTP {status}"
                )))
            }
        }
        let response_body = read_bounded_body(&mut response, MAX_RESPONSE_BYTES)
            .await
            .map_err(map_authority_response_error)?;
        let installation: InstallationAuthorityResponse = serde_json::from_slice(&response_body)
            .map_err(|_| authority_protocol("GitHub installation response JSON is invalid"))?;
        let observed_installation_id =
            crate::modules::sources::domain::GithubInstallationId::parse(installation.id).map_err(
                |error| authority_protocol(format!("GitHub installation ID is invalid: {error}")),
            )?;
        if observed_installation_id != request.installation_id {
            return Err(authority_protocol(
                "GitHub installation response changed the requested identity",
            ));
        }
        let account = installation.account.ok_or_else(|| {
            authority_protocol("GitHub installation response did not contain an account")
        })?;
        Ok(GithubProviderAuthority::available(
            observed_installation_id,
            GithubInstallationAccount {
                id: GithubAccountId::parse(account.id).map_err(|error| {
                    authority_protocol(format!("GitHub account ID is invalid: {error}"))
                })?,
                login: GithubLogin::parse(account.login).map_err(|error| {
                    authority_protocol(format!("GitHub account login is invalid: {error}"))
                })?,
                kind: GithubAccountKind::parse(&account.kind).map_err(|error| {
                    authority_protocol(format!("GitHub account type is invalid: {error}"))
                })?,
            },
            installation.suspended_at.is_some(),
        ))
    }
}

impl EnabledGithubInstallationTokenIssuer {
    fn app_jwt(
        &self,
        requested_at: DateTime<Utc>,
    ) -> Result<Zeroizing<String>, GithubInstallationTokenError> {
        let private_key = std::env::var(&self.private_key_env)
            .map(Zeroizing::new)
            .map_err(|_| GithubInstallationTokenError::Unavailable)?;
        if private_key.is_empty()
            || private_key.len() > MAX_PRIVATE_KEY_BYTES
            || private_key.contains('\0')
        {
            return Err(GithubInstallationTokenError::Unavailable);
        }
        let key = parse_private_key(private_key.as_bytes())?;
        let key_pair = match &*key {
            PrivateKeyDer::Pkcs1(value) => RsaKeyPair::from_der(value.secret_pkcs1_der()),
            PrivateKeyDer::Pkcs8(value) => RsaKeyPair::from_pkcs8(value.secret_pkcs8_der()),
            _ => return Err(GithubInstallationTokenError::Unavailable),
        }
        .map_err(|_| GithubInstallationTokenError::Unavailable)?;
        let issued_at = requested_at
            .checked_sub_signed(JWT_BACKDATE)
            .ok_or_else(|| protocol("GitHub App JWT issue time is invalid"))?;
        let expires_at = requested_at
            .checked_add_signed(JWT_FUTURE_LIFETIME)
            .ok_or_else(|| protocol("GitHub App JWT expiry is invalid"))?;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = serde_json::to_vec(&AppJwtClaims {
            iss: &self.client_id,
            iat: issued_at.timestamp(),
            exp: expires_at.timestamp(),
        })
        .map_err(|_| protocol("could not encode GitHub App JWT claims"))?;
        let signing_input = Zeroizing::new(format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims)));
        let mut signature = Zeroizing::new(vec![0_u8; key_pair.public().modulus_len()]);
        key_pair
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signing_input.as_bytes(),
                &mut signature,
            )
            .map_err(|_| GithubInstallationTokenError::Unavailable)?;
        Ok(Zeroizing::new(format!(
            "{}.{}",
            signing_input.as_str(),
            URL_SAFE_NO_PAD.encode(signature.as_slice())
        )))
    }

    async fn issue_installation_token(
        &self,
        installation_id: u64,
        requested_at: DateTime<Utc>,
        repository: Option<&str>,
    ) -> Result<InstallationTokenResponse, GithubInstallationTokenError> {
        let jwt = self.app_jwt(requested_at)?;
        let installation_id = installation_id.to_string();
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(["app", "installations", &installation_id, "access_tokens"]);
        let body = CreateInstallationTokenRequest {
            repositories: repository.map(|repository| [repository]),
            permissions: RequestedPermissions { contents: "read" },
        };
        let mut response = self
            .client
            .post(url)
            .bearer_auth(jwt.as_str())
            .json(&body)
            .send()
            .await
            .map_err(|_| GithubInstallationTokenError::Unavailable)?;
        if github_response_is_rate_limited(&response) {
            return Err(GithubInstallationTokenError::Unavailable);
        }
        match response.status() {
            StatusCode::CREATED => {}
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::UNPROCESSABLE_ENTITY => {
                return Err(GithubInstallationTokenError::Forbidden)
            }
            StatusCode::UNAUTHORIZED => return Err(GithubInstallationTokenError::Unavailable),
            status if status.is_redirection() => {
                return Err(GithubInstallationTokenError::Forbidden)
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(GithubInstallationTokenError::Unavailable)
            }
            status => {
                return Err(protocol(format!(
                    "GitHub installation-token endpoint returned unexpected HTTP {status}"
                )))
            }
        }
        let response_body = read_bounded_body(&mut response, MAX_RESPONSE_BYTES)
            .await
            .map_err(map_token_response_error)?;
        let token_response: InstallationTokenResponse = serde_json::from_slice(&response_body)
            .map_err(|_| protocol("GitHub installation-token response JSON is invalid"))?;
        let repository_selection_valid = match repository {
            Some(_) => token_response.repository_selection.as_deref() == Some("selected"),
            None => matches!(
                token_response.repository_selection.as_deref(),
                Some("all" | "selected")
            ),
        };
        if !repository_selection_valid
            || SourceProviderCredential::validate_transient(
                token_response.token.0.as_str(),
                requested_at,
                token_response.expires_at,
            )
            .is_err()
            || !token_response
                .permissions
                .iter()
                .all(|(permission, access)| {
                    matches!(
                        (permission.as_str(), access.as_str()),
                        ("contents", "read") | ("metadata", "read")
                    )
                })
            || token_response
                .permissions
                .get("contents")
                .map(String::as_str)
                != Some("read")
        {
            return Err(protocol(
                "GitHub installation token did not preserve requested repository and read-only scope",
            ));
        }
        Ok(token_response)
    }

    fn discovery_url(&self, segments: &[&str]) -> Result<Url, GithubSourceDiscoveryProviderError> {
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| discovery_protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(segments);
        Ok(url)
    }

    async fn get_discovery_json<T: DeserializeOwned>(
        &self,
        url: Url,
        token: &str,
    ) -> Result<(T, bool), GithubSourceDiscoveryProviderError> {
        let mut response = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| GithubSourceDiscoveryProviderError::Unavailable)?;
        if github_response_is_rate_limited(&response) {
            return Err(GithubSourceDiscoveryProviderError::Unavailable);
        }
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err(GithubSourceDiscoveryProviderError::Unavailable)
            }
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                return Err(GithubSourceDiscoveryProviderError::Forbidden)
            }
            status if status.is_redirection() => {
                return Err(GithubSourceDiscoveryProviderError::Forbidden)
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(GithubSourceDiscoveryProviderError::Unavailable)
            }
            status => {
                return Err(discovery_protocol(format!(
                    "GitHub discovery endpoint returned unexpected HTTP {status}"
                )))
            }
        }
        let has_next = response_has_next_link(&response)?;
        let body = read_bounded_body(&mut response, MAX_DISCOVERY_RESPONSE_BYTES)
            .await
            .map_err(map_discovery_response_error)?;
        let value = serde_json::from_slice(&body)
            .map_err(|_| discovery_protocol("GitHub discovery response JSON is invalid"))?;
        Ok((value, has_next))
    }
}

fn parse_private_key(
    pem: &[u8],
) -> Result<Zeroizing<PrivateKeyDer<'static>>, GithubInstallationTokenError> {
    let mut key = None;
    for item in rustls_pemfile::read_all(&mut BufReader::new(pem)) {
        let parsed: PrivateKeyDer<'static> =
            match item.map_err(|_| GithubInstallationTokenError::Unavailable)? {
                Item::Pkcs1Key(value) => value.into(),
                Item::Pkcs8Key(value) => value.into(),
                _ => return Err(GithubInstallationTokenError::Unavailable),
            };
        if key.is_some() {
            let _extra = Zeroizing::new(parsed);
            return Err(GithubInstallationTokenError::Unavailable);
        }
        key = Some(Zeroizing::new(parsed));
    }
    key.ok_or(GithubInstallationTokenError::Unavailable)
}

#[derive(Serialize)]
struct AppJwtClaims<'a> {
    iss: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Serialize)]
struct CreateInstallationTokenRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    repositories: Option<[&'a str; 1]>,
    permissions: RequestedPermissions<'a>,
}

#[derive(Serialize)]
struct RequestedPermissions<'a> {
    contents: &'a str,
}

struct SecretString(Zeroizing<String>);

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(Zeroizing::new)
            .map(Self)
    }
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: SecretString,
    expires_at: DateTime<Utc>,
    permissions: BTreeMap<String, String>,
    #[serde(default)]
    repository_selection: Option<String>,
}

#[derive(Deserialize)]
struct InstallationAuthorityResponse {
    id: u64,
    account: Option<InstallationAccountResponse>,
    #[serde(default)]
    suspended_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct InstallationAccountResponse {
    id: u64,
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct InstallationRepositoriesResponse {
    #[serde(rename = "total_count")]
    _total_count: usize,
    repositories: Vec<RepositoryResponse>,
}

#[derive(Deserialize)]
struct RepositoryResponse {
    name: String,
    full_name: String,
    html_url: String,
    #[serde(default)]
    default_branch: Option<String>,
    private: bool,
    fork: bool,
    archived: bool,
    disabled: bool,
}

#[derive(Deserialize)]
struct BranchResponse {
    name: String,
    commit: ReferenceCommitResponse,
    protected: bool,
}

#[derive(Deserialize)]
struct TagResponse {
    name: String,
    commit: ReferenceCommitResponse,
}

#[derive(Deserialize)]
struct ReferenceCommitResponse {
    sha: String,
}

fn discovered_repository(
    value: RepositoryResponse,
) -> Result<Option<GithubDiscoveredRepository>, GithubSourceDiscoveryProviderError> {
    let Ok(repository) =
        crate::modules::sources::domain::GitRepository::parse(GitProvider::Github, &value.html_url)
    else {
        return Ok(None);
    };
    let (owner, name) = repository.owner_and_name().ok_or_else(|| {
        discovery_protocol("GitHub repository discovery coordinates are unavailable")
    })?;
    let expected_full_name = format!("{owner}/{name}");
    if !value.name.eq_ignore_ascii_case(name)
        || !value.full_name.eq_ignore_ascii_case(&expected_full_name)
    {
        return Err(discovery_protocol(
            "GitHub repository discovery response changed repository identity",
        ));
    }
    let Some(default_branch) = value.default_branch else {
        return Ok(None);
    };
    if GitReference::parse("branch", default_branch.clone()).is_err() {
        return Ok(None);
    }
    let discovered = GithubDiscoveredRepository {
        repository,
        default_branch,
        private: value.private,
        fork: value.fork,
        archived: value.archived,
        disabled: value.disabled,
    };
    discovered
        .validate()
        .map_err(|_| discovery_protocol("GitHub repository discovery response is invalid"))?;
    Ok(Some(discovered))
}

fn discovered_branch(
    value: BranchResponse,
) -> Result<Option<GithubDiscoveredReference>, GithubSourceDiscoveryProviderError> {
    discovered_reference(
        GithubDiscoveredReferenceKind::Branch,
        value.name,
        value.commit.sha,
        Some(value.protected),
    )
}

fn discovered_tag(
    value: TagResponse,
) -> Result<Option<GithubDiscoveredReference>, GithubSourceDiscoveryProviderError> {
    discovered_reference(
        GithubDiscoveredReferenceKind::Tag,
        value.name,
        value.commit.sha,
        None,
    )
}

fn discovered_reference(
    kind: GithubDiscoveredReferenceKind,
    name: String,
    commit_sha: String,
    protected: Option<bool>,
) -> Result<Option<GithubDiscoveredReference>, GithubSourceDiscoveryProviderError> {
    if GitReference::parse(kind.as_str(), name.clone()).is_err() {
        return Ok(None);
    }
    let reference = GithubDiscoveredReference {
        kind,
        name,
        commit_sha: GitCommitSha::parse(commit_sha)
            .map_err(|_| discovery_protocol("GitHub discovered commit SHA is invalid"))?,
        protected,
    };
    reference
        .validate()
        .map_err(|_| discovery_protocol("GitHub discovered reference is invalid"))?;
    Ok(Some(reference))
}

enum GithubResponseReadError {
    Unavailable,
    TooLarge,
}

async fn read_bounded_body(
    response: &mut reqwest::Response,
    maximum_bytes: u64,
) -> Result<Zeroizing<Vec<u8>>, GithubResponseReadError> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum_bytes)
    {
        return Err(GithubResponseReadError::TooLarge);
    }
    let mut body = Zeroizing::new(Vec::with_capacity(
        response.content_length().unwrap_or(0).min(maximum_bytes) as usize,
    ));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| GithubResponseReadError::Unavailable)?
    {
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length as u64 > maximum_bytes)
        {
            return Err(GithubResponseReadError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn response_has_next_link(
    response: &reqwest::Response,
) -> Result<bool, GithubSourceDiscoveryProviderError> {
    let Some(value) = response.headers().get(LINK) else {
        return Ok(false);
    };
    let value = value
        .to_str()
        .map_err(|_| discovery_protocol("GitHub pagination Link header is invalid"))?;
    if value.is_empty() || value.len() > MAX_LINK_HEADER_BYTES {
        return Err(discovery_protocol(
            "GitHub pagination Link header exceeded its bound",
        ));
    }
    let mut has_next = false;
    for link in value.split(',') {
        let mut parts = link.trim().split(';');
        let target = parts
            .next()
            .and_then(|value| value.strip_prefix('<'))
            .and_then(|value| value.strip_suffix('>'))
            .ok_or_else(|| discovery_protocol("GitHub pagination Link target is invalid"))?;
        let target = Url::parse(target)
            .map_err(|_| discovery_protocol("GitHub pagination Link target is invalid"))?;
        if !matches!(target.scheme(), "https" | "http")
            || target.host_str().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.fragment().is_some()
        {
            return Err(discovery_protocol(
                "GitHub pagination Link target is invalid",
            ));
        }
        for parameter in parts {
            if parameter.trim() == "rel=\"next\"" {
                has_next = true;
            }
        }
    }
    Ok(has_next)
}

fn github_response_is_rate_limited(response: &reqwest::Response) -> bool {
    response.status() == StatusCode::TOO_MANY_REQUESTS
        || (response.status() == StatusCode::FORBIDDEN
            && (response.headers().contains_key(RETRY_AFTER)
                || response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|value| value.to_str().ok())
                    == Some("0")))
}

fn map_token_response_error(error: GithubResponseReadError) -> GithubInstallationTokenError {
    match error {
        GithubResponseReadError::Unavailable => GithubInstallationTokenError::Unavailable,
        GithubResponseReadError::TooLarge => protocol("GitHub response exceeded the size limit"),
    }
}

fn map_authority_response_error(
    error: GithubResponseReadError,
) -> GithubInstallationAuthorityError {
    match error {
        GithubResponseReadError::Unavailable => GithubInstallationAuthorityError::Unavailable,
        GithubResponseReadError::TooLarge => {
            authority_protocol("GitHub response exceeded the size limit")
        }
    }
}

fn map_discovery_response_error(
    error: GithubResponseReadError,
) -> GithubSourceDiscoveryProviderError {
    match error {
        GithubResponseReadError::Unavailable => GithubSourceDiscoveryProviderError::Unavailable,
        GithubResponseReadError::TooLarge => {
            discovery_protocol("GitHub discovery response exceeded the size limit")
        }
    }
}

fn map_discovery_token_error(
    error: GithubInstallationTokenError,
) -> GithubSourceDiscoveryProviderError {
    match error {
        GithubInstallationTokenError::NotConfigured => {
            GithubSourceDiscoveryProviderError::NotConfigured
        }
        GithubInstallationTokenError::Forbidden => GithubSourceDiscoveryProviderError::Forbidden,
        GithubInstallationTokenError::Unavailable => {
            GithubSourceDiscoveryProviderError::Unavailable
        }
        GithubInstallationTokenError::Protocol(message) => {
            GithubSourceDiscoveryProviderError::Protocol(message)
        }
    }
}

fn map_authority_signing_error(
    error: GithubInstallationTokenError,
) -> GithubInstallationAuthorityError {
    match error {
        GithubInstallationTokenError::NotConfigured => {
            GithubInstallationAuthorityError::NotConfigured
        }
        GithubInstallationTokenError::Unavailable => GithubInstallationAuthorityError::Unavailable,
        GithubInstallationTokenError::Forbidden => GithubInstallationAuthorityError::Unavailable,
        GithubInstallationTokenError::Protocol(message) => {
            GithubInstallationAuthorityError::Protocol(message)
        }
    }
}

fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_endpoint(url: &Url, allow_http: bool) -> bool {
    matches!(url.path(), "" | "/")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || allow_http && url.scheme() == "http")
}

fn protocol(message: impl Into<String>) -> GithubInstallationTokenError {
    GithubInstallationTokenError::Protocol(message.into())
}

fn authority_protocol(message: impl Into<String>) -> GithubInstallationAuthorityError {
    GithubInstallationAuthorityError::Protocol(message.into())
}

fn discovery_protocol(message: impl Into<String>) -> GithubSourceDiscoveryProviderError {
    GithubSourceDiscoveryProviderError::Protocol(message.into())
}

#[cfg(test)]
#[path = "github_installation_token_issuer_tests.rs"]
mod tests;
