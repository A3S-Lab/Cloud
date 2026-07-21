use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::{
    GitProvider, GithubInstallationTokenError, GithubInstallationTokenRequest,
    IGithubInstallationTokenService, SourceProviderCredential,
};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::{Client, StatusCode};
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use rustls::pki_types::PrivateKeyDer;
use rustls_pemfile::Item;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const GITHUB_API_URL: &str = "https://api.github.com/";
const GITHUB_API_VERSION: &str = "2022-11-28";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
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
        let jwt = enabled.app_jwt(requested_at)?;
        let installation_id = request.installation_id.as_u64().to_string();
        let mut url = enabled.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| protocol("GitHub API URL cannot contain path segments"))?
            .clear()
            .extend(["app", "installations", &installation_id, "access_tokens"]);
        let body = CreateInstallationTokenRequest {
            repositories: [repository_name],
            permissions: RequestedPermissions { contents: "read" },
        };
        let mut response = enabled
            .client
            .post(url)
            .bearer_auth(jwt.as_str())
            .json(&body)
            .send()
            .await
            .map_err(|_| GithubInstallationTokenError::Unavailable)?;
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
        let response_body = read_bounded_body(&mut response).await?;
        let token_response: InstallationTokenResponse = serde_json::from_slice(&response_body)
            .map_err(|_| protocol("GitHub installation-token response JSON is invalid"))?;
        if token_response.repository_selection.as_deref() != Some("selected")
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
                "GitHub installation token did not preserve repository and read-only scope",
            ));
        }
        SourceProviderCredential::new(
            &request.repository,
            token_response.token.0,
            requested_at,
            token_response.expires_at,
        )
        .map_err(|error| protocol(format!("GitHub installation token is invalid: {error}")))
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
    repositories: [&'a str; 1],
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

async fn read_bounded_body(
    response: &mut reqwest::Response,
) -> Result<Zeroizing<Vec<u8>>, GithubInstallationTokenError> {
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
        .map_err(|_| GithubInstallationTokenError::Unavailable)?
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

#[cfg(test)]
#[path = "github_installation_token_issuer_tests.rs"]
mod tests;
