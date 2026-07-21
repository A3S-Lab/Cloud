use crate::modules::sources::domain::{
    GithubAccountId, GithubAccountKind, GithubAppAuthorizationError,
    GithubInstallationVerificationRequest, GithubLogin, IGithubAppAuthorizationService,
    VerifiedGithubInstallation,
};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::{Client, StatusCode};
use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_INSTALL_URL: &str = "https://github.com/apps/";
const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_API_URL: &str = "https://api.github.com/";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
const INSTALLATIONS_PER_PAGE: usize = 100;
const MAX_INSTALLATION_PAGES: usize = 10;

#[derive(Clone)]
pub struct GithubAppClient {
    enabled: Option<EnabledGithubAppClient>,
}

#[derive(Clone)]
struct EnabledGithubAppClient {
    client: Client,
    app_slug: String,
    client_id: String,
    client_secret_env: String,
    callback_url: Url,
    install_base: Url,
    authorize_url: Url,
    token_url: Url,
    api_base: Url,
}

impl GithubAppClient {
    pub const fn disabled() -> Self {
        Self { enabled: None }
    }

    pub fn new(
        timeout: Duration,
        app_slug: impl Into<String>,
        client_id: impl Into<String>,
        client_secret_env: impl Into<String>,
        callback_url: impl AsRef<str>,
    ) -> Result<Self, String> {
        Self::with_endpoints(
            timeout,
            app_slug.into(),
            client_id.into(),
            client_secret_env.into(),
            Url::parse(callback_url.as_ref())
                .map_err(|error| format!("GitHub App callback URL is invalid: {error}"))?,
            Url::parse(GITHUB_INSTALL_URL)
                .map_err(|error| format!("GitHub install URL is invalid: {error}"))?,
            Url::parse(GITHUB_AUTHORIZE_URL)
                .map_err(|error| format!("GitHub authorization URL is invalid: {error}"))?,
            Url::parse(GITHUB_TOKEN_URL)
                .map_err(|error| format!("GitHub token URL is invalid: {error}"))?,
            Url::parse(GITHUB_API_URL)
                .map_err(|error| format!("GitHub API URL is invalid: {error}"))?,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_endpoints(
        timeout: Duration,
        app_slug: String,
        client_id: String,
        client_secret_env: String,
        callback_url: Url,
        install_base: Url,
        authorize_url: Url,
        token_url: Url,
        api_base: Url,
        allow_http: bool,
    ) -> Result<Self, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err("GitHub request timeout must be between 1 ms and 60 seconds".into());
        }
        if !valid_slug(&app_slug)
            || !valid_client_id(&client_id)
            || !valid_env_name(&client_secret_env)
            || !valid_callback_url(&callback_url, allow_http)
            || !valid_endpoint(&install_base, allow_http)
            || !valid_endpoint(&authorize_url, allow_http)
            || !valid_endpoint(&token_url, allow_http)
            || !valid_endpoint(&api_base, allow_http)
        {
            return Err("GitHub App authorization configuration is invalid".into());
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
            .map_err(|error| format!("could not build GitHub App client: {error}"))?;
        Ok(Self {
            enabled: Some(EnabledGithubAppClient {
                client,
                app_slug,
                client_id,
                client_secret_env,
                callback_url,
                install_base,
                authorize_url,
                token_url,
                api_base,
            }),
        })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn for_test(
        timeout: Duration,
        app_slug: &str,
        client_id: &str,
        client_secret_env: &str,
        callback_url: Url,
        install_base: Url,
        authorize_url: Url,
        token_url: Url,
        api_base: Url,
    ) -> Result<Self, String> {
        Self::with_endpoints(
            timeout,
            app_slug.into(),
            client_id.into(),
            client_secret_env.into(),
            callback_url,
            install_base,
            authorize_url,
            token_url,
            api_base,
            true,
        )
    }

    fn require_enabled(&self) -> Result<&EnabledGithubAppClient, GithubAppAuthorizationError> {
        self.enabled
            .as_ref()
            .ok_or(GithubAppAuthorizationError::NotConfigured)
    }
}

#[async_trait]
impl IGithubAppAuthorizationService for GithubAppClient {
    fn installation_url(&self, state: &str) -> Result<String, GithubAppAuthorizationError> {
        let enabled = self.require_enabled()?;
        require_flow_value(state, "installation state")?;
        let mut url = enabled.install_base.clone();
        url.path_segments_mut()
            .map_err(|_| protocol("GitHub install URL cannot contain path segments"))?
            .pop_if_empty()
            .push(&enabled.app_slug)
            .push("installations")
            .push("new");
        url.query_pairs_mut().append_pair("state", state);
        Ok(url.into())
    }

    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<String, GithubAppAuthorizationError> {
        let enabled = self.require_enabled()?;
        require_flow_value(state, "OAuth state")?;
        require_flow_value(pkce_challenge, "PKCE challenge")?;
        let mut url = enabled.authorize_url.clone();
        url.query_pairs_mut()
            .append_pair("client_id", &enabled.client_id)
            .append_pair("redirect_uri", enabled.callback_url.as_str())
            .append_pair("state", state)
            .append_pair("code_challenge", pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    async fn verify_installation(
        &self,
        request: GithubInstallationVerificationRequest,
    ) -> Result<VerifiedGithubInstallation, GithubAppAuthorizationError> {
        let enabled = self.require_enabled()?;
        let token = enabled
            .exchange_code(&request.code, &request.pkce_verifier)
            .await?;
        let user: GithubUserResponse = enabled
            .get_api_json(enabled.api_url(&["user"])?, &token)
            .await?;
        let user_id = GithubAccountId::parse(user.id)
            .map_err(|error| protocol(format!("GitHub user ID is invalid: {error}")))?;
        let user_login = GithubLogin::parse(user.login)
            .map_err(|error| protocol(format!("GitHub user login is invalid: {error}")))?;
        let installation = enabled
            .installation_for_user(&token, request.installation_id.as_u64())
            .await?;
        let installation_id =
            crate::modules::sources::domain::GithubInstallationId::parse(installation.id)
                .map_err(|error| protocol(format!("GitHub installation ID is invalid: {error}")))?;
        let account = installation
            .account
            .ok_or_else(|| protocol("GitHub installation response did not contain an account"))?;
        Ok(VerifiedGithubInstallation {
            installation_id,
            account_id: GithubAccountId::parse(account.id)
                .map_err(|error| protocol(format!("GitHub account ID is invalid: {error}")))?,
            account_login: GithubLogin::parse(account.login)
                .map_err(|error| protocol(format!("GitHub account login is invalid: {error}")))?,
            account_kind: GithubAccountKind::parse(&account.kind)
                .map_err(|error| protocol(format!("GitHub account type is invalid: {error}")))?,
            user_id,
            user_login,
        })
    }
}

impl EnabledGithubAppClient {
    async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: &str,
    ) -> Result<SecretString, GithubAppAuthorizationError> {
        let client_secret = std::env::var(&self.client_secret_env)
            .map(Zeroizing::new)
            .map_err(|_| GithubAppAuthorizationError::Unavailable)?;
        if client_secret.is_empty()
            || client_secret.len() > 1024
            || client_secret.contains(['\0', '\r', '\n'])
        {
            return Err(GithubAppAuthorizationError::Unavailable);
        }
        let mut response = self
            .client
            .post(self.token_url.clone())
            .header(ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("code", code),
                ("redirect_uri", self.callback_url.as_str()),
                ("code_verifier", pkce_verifier),
            ])
            .send()
            .await
            .map_err(|_| GithubAppAuthorizationError::Unavailable)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(GithubAppAuthorizationError::Rejected)
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(GithubAppAuthorizationError::Unavailable)
            }
            status => {
                return Err(protocol(format!(
                    "GitHub token endpoint returned unexpected HTTP {status}"
                )))
            }
        }
        let body = read_bounded_body(&mut response).await?;
        let token_response: GithubTokenResponse = serde_json::from_slice(&body)
            .map_err(|_| protocol("GitHub token response JSON is invalid"))?;
        if token_response.error.is_some() {
            return Err(GithubAppAuthorizationError::Rejected);
        }
        if !token_response
            .token_type
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
        {
            return Err(protocol("GitHub token response type is invalid"));
        }
        let access_token = token_response
            .access_token
            .ok_or_else(|| protocol("GitHub token response omitted the access token"))?;
        if access_token.0.is_empty()
            || access_token.0.len() > 2048
            || access_token.0.contains(['\0', '\r', '\n'])
        {
            return Err(protocol("GitHub access token is invalid"));
        }
        Ok(access_token)
    }

    async fn installation_for_user(
        &self,
        token: &SecretString,
        expected_installation_id: u64,
    ) -> Result<GithubInstallationResponse, GithubAppAuthorizationError> {
        for page in 1..=MAX_INSTALLATION_PAGES {
            let mut url = self.api_url(&["user", "installations"])?;
            url.query_pairs_mut()
                .append_pair("per_page", &INSTALLATIONS_PER_PAGE.to_string())
                .append_pair("page", &page.to_string());
            let response: GithubInstallationsResponse = self.get_api_json(url, token).await?;
            if response.installations.len() > INSTALLATIONS_PER_PAGE {
                return Err(protocol(
                    "GitHub installations response exceeded the requested page size",
                ));
            }
            if let Some(installation) = response
                .installations
                .into_iter()
                .find(|installation| installation.id == expected_installation_id)
            {
                return Ok(installation);
            }
            if page
                .checked_mul(INSTALLATIONS_PER_PAGE)
                .is_none_or(|seen| seen >= response.total_count)
            {
                break;
            }
        }
        Err(GithubAppAuthorizationError::Forbidden)
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url, GithubAppAuthorizationError> {
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(segments);
        Ok(url)
    }

    async fn get_api_json<T: DeserializeOwned>(
        &self,
        url: Url,
        token: &SecretString,
    ) -> Result<T, GithubAppAuthorizationError> {
        let mut response = self
            .client
            .get(url)
            .bearer_auth(token.0.as_str())
            .send()
            .await
            .map_err(|_| GithubAppAuthorizationError::Unavailable)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(GithubAppAuthorizationError::Rejected),
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                return Err(GithubAppAuthorizationError::Forbidden)
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(GithubAppAuthorizationError::Unavailable)
            }
            status => {
                return Err(protocol(format!(
                    "GitHub API returned unexpected HTTP {status}"
                )))
            }
        }
        let body = read_bounded_body(&mut response).await?;
        serde_json::from_slice(&body).map_err(|_| protocol("GitHub API response JSON is invalid"))
    }
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
struct GithubTokenResponse {
    #[serde(default)]
    access_token: Option<SecretString>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default, rename = "refresh_token")]
    _refresh_token: Option<SecretString>,
    #[serde(default)]
    error: Option<SecretString>,
}

#[derive(Deserialize)]
struct GithubUserResponse {
    id: u64,
    login: String,
}

#[derive(Deserialize)]
struct GithubInstallationsResponse {
    total_count: usize,
    installations: Vec<GithubInstallationResponse>,
}

#[derive(Deserialize)]
struct GithubInstallationResponse {
    id: u64,
    account: Option<GithubAccountResponse>,
}

#[derive(Deserialize)]
struct GithubAccountResponse {
    id: u64,
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

async fn read_bounded_body(
    response: &mut reqwest::Response,
) -> Result<Zeroizing<Vec<u8>>, GithubAppAuthorizationError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(protocol("GitHub response exceeded the size limit"));
    }
    let mut body = Zeroizing::new(Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(MAX_RESPONSE_BYTES) as usize,
    ));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| GithubAppAuthorizationError::Unavailable)?
    {
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length as u64 > MAX_RESPONSE_BYTES)
        {
            return Err(protocol("GitHub response exceeded the size limit"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn require_flow_value(value: &str, label: &str) -> Result<(), GithubAppAuthorizationError> {
    if value.len() != 43
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(protocol(format!("GitHub {label} is invalid")));
    }
    Ok(())
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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

fn valid_callback_url(url: &Url, allow_http: bool) -> bool {
    url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || allow_http && url.scheme() == "http")
        && url.path() == "/api/v1/source-connections/github/callback"
}

fn valid_endpoint(url: &Url, allow_http: bool) -> bool {
    url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || allow_http && url.scheme() == "http")
}

fn protocol(message: impl Into<String>) -> GithubAppAuthorizationError {
    GithubAppAuthorizationError::Protocol(message.into())
}

#[cfg(test)]
#[path = "github_app_client_tests.rs"]
mod tests;
