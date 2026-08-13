use crate::config::OidcProviderConfig;
use crate::modules::identity::domain::services::{
    IOidcProviderService, OidcAuthorization, OidcAuthorizationRequest, OidcCodeVerificationRequest,
    OidcProviderError, VerifiedOidcIdentity,
};
use crate::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::shared_kernel::application::{pkce_s256_challenge, validate_oauth_flow_secret};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use futures_util::StreamExt;
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreClientAuthMethod, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, CoreResponseType,
};
use openidconnect::{
    AccessTokenHash, AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl,
    Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const MAX_OIDC_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_OIDC_CODE_BYTES: usize = 4096;
const MAX_OIDC_CLIENT_SECRET_BYTES: usize = 8192;
const MAX_ID_TOKEN_AGE_SECONDS: i64 = 900;
const CLOCK_SKEW_SECONDS: i64 = 60;

#[derive(Clone)]
pub struct OpenIdConnectProviderService {
    providers: Arc<BTreeMap<OidcProviderKey, Provider>>,
}

#[derive(Clone)]
struct Provider {
    key: OidcProviderKey,
    issuer: OidcIssuer,
    client_id: String,
    client_secret_env: String,
    callback_url: String,
    config_digest: Sha256Digest,
    flow_lifetime: chrono::Duration,
    login_token_lifetime: chrono::Duration,
    client: BoundedOidcHttpClient,
}

#[derive(Clone)]
struct BoundedOidcHttpClient {
    inner: reqwest::Client,
}

struct DiscoveredProvider {
    metadata: CoreProviderMetadata,
    token_auth_type: AuthType,
}

impl OpenIdConnectProviderService {
    pub fn new(configs: &[OidcProviderConfig]) -> Result<Self, String> {
        let providers = configs
            .iter()
            .map(|config| {
                let provider = Provider::new(config)?;
                Ok((provider.key.clone(), provider))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        if providers.len() != configs.len() {
            return Err("OIDC provider keys must be unique".into());
        }
        Ok(Self {
            providers: Arc::new(providers),
        })
    }

    fn provider(&self, key: &OidcProviderKey) -> Result<&Provider, OidcProviderError> {
        self.providers
            .get(key)
            .ok_or(OidcProviderError::NotConfigured)
    }
}

impl Provider {
    fn new(config: &OidcProviderConfig) -> Result<Self, String> {
        let timeout = Duration::from_millis(config.request_timeout_ms);
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err("OIDC request timeout must be between 1 ms and 60 seconds".into());
        }
        let inner = secure_http_client(timeout, None)?;
        Self::with_http_client(config, BoundedOidcHttpClient { inner })
    }

    fn with_http_client(
        config: &OidcProviderConfig,
        client: BoundedOidcHttpClient,
    ) -> Result<Self, String> {
        let key = OidcProviderKey::parse(config.key.clone())?;
        let issuer = OidcIssuer::parse(config.issuer.clone())?;
        let callback = Url::parse(&config.callback_url)
            .map_err(|error| format!("OIDC callback URL is invalid: {error}"))?;
        if !valid_endpoint(&callback) {
            return Err("OIDC callback URL is unsafe".into());
        }
        let flow_lifetime =
            bounded_lifetime(config.flow_ttl_ms, 60_000, 900_000, "OIDC flow lifetime")?;
        let login_token_lifetime = bounded_lifetime(
            config.login_token_ttl_ms,
            300_000,
            86_400_000,
            "OIDC login token lifetime",
        )?;
        Ok(Self {
            key,
            issuer,
            client_id: config.client_id.clone(),
            client_secret_env: config.client_secret_env.clone(),
            callback_url: config.callback_url.clone(),
            config_digest: config.public_config_digest()?,
            flow_lifetime,
            login_token_lifetime,
            client,
        })
    }

    #[cfg(test)]
    fn for_test(config: &OidcProviderConfig, root: reqwest::Certificate) -> Result<Self, String> {
        let timeout = Duration::from_millis(config.request_timeout_ms);
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err("OIDC request timeout must be between 1 ms and 60 seconds".into());
        }
        let inner = secure_http_client(timeout, Some(root))?;
        Self::with_http_client(config, BoundedOidcHttpClient { inner })
    }

    async fn discover(&self) -> Result<DiscoveredProvider, OidcProviderError> {
        let issuer = IssuerUrl::new(self.issuer.as_str().to_owned())
            .map_err(|_| protocol("configured issuer is invalid"))?;
        let mut metadata = CoreProviderMetadata::discover_async(issuer, &self.client)
            .await
            .map_err(map_discovery_error)?;
        require_safe_endpoint(metadata.authorization_endpoint().url(), "authorization")?;
        let token_endpoint = metadata
            .token_endpoint()
            .ok_or_else(|| protocol("discovery document omitted the token endpoint"))?;
        require_safe_endpoint(token_endpoint.url(), "token")?;
        require_safe_endpoint(metadata.jwks_uri().url(), "JWKS")?;
        if metadata.jwks().keys().is_empty() {
            return Err(protocol(
                "discovered JWKS contained no supported verification key",
            ));
        }
        if !metadata
            .response_types_supported()
            .iter()
            .any(|types| types.as_slice() == [CoreResponseType::Code])
        {
            return Err(protocol(
                "discovery document does not support the authorization code response type",
            ));
        }
        let allowed_algorithms = metadata
            .id_token_signing_alg_values_supported()
            .iter()
            .filter(|algorithm| is_allowed_signing_algorithm(algorithm))
            .cloned()
            .collect::<Vec<_>>();
        if allowed_algorithms.is_empty() {
            return Err(protocol(
                "discovery document advertised no supported asymmetric ID token signing algorithm",
            ));
        }
        metadata = metadata.set_id_token_signing_alg_values_supported(allowed_algorithms);
        let token_auth_type = match metadata.token_endpoint_auth_methods_supported() {
            None => AuthType::BasicAuth,
            Some(methods) if methods.contains(&CoreClientAuthMethod::ClientSecretBasic) => {
                AuthType::BasicAuth
            }
            Some(methods) if methods.contains(&CoreClientAuthMethod::ClientSecretPost) => {
                AuthType::RequestBody
            }
            Some(_) => {
                return Err(protocol(
                    "discovery document advertised no supported client-secret token authentication method",
                ))
            }
        };
        Ok(DiscoveredProvider {
            metadata,
            token_auth_type,
        })
    }

    fn client_secret(&self) -> Result<Zeroizing<String>, OidcProviderError> {
        let secret = std::env::var(&self.client_secret_env)
            .map(Zeroizing::new)
            .map_err(|_| OidcProviderError::CredentialUnavailable)?;
        if secret.is_empty()
            || secret.len() > MAX_OIDC_CLIENT_SECRET_BYTES
            || secret.contains(['\0', '\r', '\n'])
        {
            return Err(OidcProviderError::CredentialUnavailable);
        }
        Ok(secret)
    }
}

fn secure_http_client(
    timeout: Duration,
    root: Option<reqwest::Certificate>,
) -> Result<reqwest::Client, String> {
    // Reqwest's rustls backend consults the process provider when multiple
    // providers are linked. Cloud standardizes on ring for its TLS surfaces.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .https_only(true)
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("a3s-cloud-control-plane");
    if let Some(root) = root {
        builder = builder.add_root_certificate(root);
    }
    builder
        .build()
        .map_err(|error| format!("could not build OIDC HTTP client: {error}"))
}

#[async_trait]
impl IOidcProviderService for OpenIdConnectProviderService {
    async fn authorization_url(
        &self,
        request: OidcAuthorizationRequest,
    ) -> Result<OidcAuthorization, OidcProviderError> {
        let provider = self.provider(&request.provider_key)?;
        let state = validate_oauth_flow_secret(request.state, "OIDC state")
            .map_err(|error| OidcProviderError::Protocol(error.to_string()))?;
        let nonce = validate_oauth_flow_secret(request.nonce, "OIDC nonce")
            .map_err(|error| OidcProviderError::Protocol(error.to_string()))?;
        let verifier = validate_oauth_flow_secret(request.pkce_verifier, "OIDC PKCE verifier")
            .map_err(|error| OidcProviderError::Protocol(error.to_string()))?;
        let discovered = provider.discover().await?;
        let client = CoreClient::from_provider_metadata(
            discovered.metadata,
            ClientId::new(provider.client_id.clone()),
            None,
        )
        .set_redirect_uri(
            RedirectUrl::new(provider.callback_url.clone())
                .map_err(|_| protocol("configured callback URL is invalid"))?,
        );
        let state_value = state.to_string();
        let nonce_value = nonce.to_string();
        let pkce = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
            verifier.to_string(),
        ));
        debug_assert_eq!(pkce.as_str(), pkce_s256_challenge(&verifier));
        let (authorization_url, returned_state, returned_nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state_value),
                move || Nonce::new(nonce_value),
            )
            .set_pkce_challenge(pkce)
            .url();
        if returned_state.secret() != state.as_str() || returned_nonce.secret() != nonce.as_str() {
            return Err(protocol("OIDC library changed caller-bound flow material"));
        }
        Ok(OidcAuthorization {
            authorization_url: authorization_url.into(),
            provider_key: provider.key.clone(),
            issuer: provider.issuer.clone(),
            provider_config_digest: provider.config_digest.clone(),
            flow_lifetime: provider.flow_lifetime,
        })
    }

    async fn verify_code(
        &self,
        request: OidcCodeVerificationRequest,
    ) -> Result<VerifiedOidcIdentity, OidcProviderError> {
        let provider = self.provider(&request.provider_key)?;
        let code = validate_authorization_code(request.code)?;
        let nonce = validate_oauth_flow_secret(request.nonce, "OIDC nonce")
            .map_err(|error| OidcProviderError::Protocol(error.to_string()))?;
        let verifier = validate_oauth_flow_secret(request.pkce_verifier, "OIDC PKCE verifier")
            .map_err(|error| OidcProviderError::Protocol(error.to_string()))?;
        let discovered = provider.discover().await?;
        let secret = provider.client_secret()?;
        let client = CoreClient::from_provider_metadata(
            discovered.metadata,
            ClientId::new(provider.client_id.clone()),
            Some(ClientSecret::new(secret.to_string())),
        )
        .set_auth_type(discovered.token_auth_type)
        .set_redirect_uri(
            RedirectUrl::new(provider.callback_url.clone())
                .map_err(|_| protocol("configured callback URL is invalid"))?,
        );
        let response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|_| protocol("discovery document cannot exchange an authorization code"))?
            .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_string()))
            .request_async(&provider.client)
            .await
            .map_err(map_token_error)?;
        let id_token = response
            .extra_fields()
            .id_token()
            .ok_or_else(|| protocol("token response omitted the ID token"))?;
        if !is_allowed_signing_algorithm(
            id_token
                .signing_alg()
                .map_err(|_| protocol("ID token signing algorithm is invalid"))?,
        ) {
            return Err(protocol("ID token signing algorithm is not asymmetric"));
        }
        let now = chrono::Utc::now();
        let oldest = now - chrono::Duration::seconds(MAX_ID_TOKEN_AGE_SECONDS);
        let newest = now + chrono::Duration::seconds(CLOCK_SKEW_SECONDS);
        let id_token_verifier = client
            .id_token_verifier()
            .set_time_fn(move || now)
            .set_issue_time_verifier_fn(move |issued_at| {
                if issued_at < oldest || issued_at > newest {
                    Err("ID token issue time is outside the accepted flow window".into())
                } else {
                    Ok(())
                }
            });
        let claims = id_token
            .claims(&id_token_verifier, &Nonce::new(nonce.to_string()))
            .map_err(|_| OidcProviderError::Rejected)?;
        if claims.audiences().len() != 1
            || claims.audiences()[0].as_str() != provider.client_id
            || claims
                .authorized_party()
                .is_some_and(|party| party.as_str() != provider.client_id)
        {
            return Err(OidcProviderError::Rejected);
        }
        if let Some(expected_hash) = claims.access_token_hash() {
            let actual_hash = AccessTokenHash::from_token(
                response.access_token(),
                id_token
                    .signing_alg()
                    .map_err(|_| OidcProviderError::Rejected)?,
                id_token
                    .signing_key(&id_token_verifier)
                    .map_err(|_| OidcProviderError::Rejected)?,
            )
            .map_err(|_| OidcProviderError::Rejected)?;
            if actual_hash != *expected_hash {
                return Err(OidcProviderError::Rejected);
            }
        }
        let subject = ExternalIdentitySubject::parse(claims.subject().as_str().to_owned())
            .map_err(|error| protocol(format!("ID token subject is invalid: {error}")))?;
        Ok(VerifiedOidcIdentity {
            provider_key: provider.key.clone(),
            issuer: provider.issuer.clone(),
            provider_config_digest: provider.config_digest.clone(),
            subject,
            login_token_lifetime: provider.login_token_lifetime,
        })
    }
}

impl<'c> openidconnect::AsyncHttpClient<'c> for BoundedOidcHttpClient {
    type Error = BoundedOidcHttpError;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<openidconnect::HttpResponse, Self::Error>>
                + Send
                + Sync
                + 'c,
        >,
    >;

    fn call(&'c self, request: openidconnect::HttpRequest) -> Self::Future {
        Box::pin(async move {
            let request = reqwest::Request::try_from(request)
                .map_err(|error| BoundedOidcHttpError::Request(error.to_string()))?;
            if !valid_endpoint(request.url()) {
                return Err(BoundedOidcHttpError::UnsafeUrl);
            }
            let response = self
                .inner
                .execute(request)
                .await
                .map_err(|error| BoundedOidcHttpError::Request(error.to_string()))?;
            if response
                .content_length()
                .is_some_and(|length| length > MAX_OIDC_RESPONSE_BYTES)
            {
                return Err(BoundedOidcHttpError::ResponseTooLarge);
            }
            let status = response.status();
            let version = response.version();
            let headers = response.headers().clone();
            let mut body = Vec::new();
            let mut chunks = response.bytes_stream();
            while let Some(chunk) = chunks.next().await {
                let chunk =
                    chunk.map_err(|error| BoundedOidcHttpError::Request(error.to_string()))?;
                if body
                    .len()
                    .checked_add(chunk.len())
                    .is_none_or(|length| length as u64 > MAX_OIDC_RESPONSE_BYTES)
                {
                    return Err(BoundedOidcHttpError::ResponseTooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            let mut builder = openidconnect::http::Response::builder()
                .status(status)
                .version(version);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder
                .body(body)
                .map_err(|error| BoundedOidcHttpError::Request(error.to_string()))
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum BoundedOidcHttpError {
    #[error("OIDC endpoint URL is unsafe")]
    UnsafeUrl,
    #[error("OIDC response exceeded the byte bound")]
    ResponseTooLarge,
    #[error("OIDC request failed: {0}")]
    Request(String),
}

fn valid_endpoint(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn bounded_lifetime(
    milliseconds: u64,
    minimum: u64,
    maximum: u64,
    label: &str,
) -> Result<chrono::Duration, String> {
    if !(minimum..=maximum).contains(&milliseconds) {
        return Err(format!(
            "{label} must be between {minimum} ms and {maximum} ms"
        ));
    }
    let milliseconds =
        i64::try_from(milliseconds).map_err(|_| format!("{label} exceeds the supported range"))?;
    Ok(chrono::Duration::milliseconds(milliseconds))
}

fn require_safe_endpoint(url: &Url, label: &str) -> Result<(), OidcProviderError> {
    if valid_endpoint(url) {
        Ok(())
    } else {
        Err(protocol(format!(
            "discovered {label} endpoint must be an HTTPS URL without credentials or fragment"
        )))
    }
}

fn is_allowed_signing_algorithm(algorithm: &CoreJwsSigningAlgorithm) -> bool {
    matches!(
        algorithm,
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256
            | CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384
            | CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512
            | CoreJwsSigningAlgorithm::RsaSsaPssSha256
            | CoreJwsSigningAlgorithm::RsaSsaPssSha384
            | CoreJwsSigningAlgorithm::RsaSsaPssSha512
            | CoreJwsSigningAlgorithm::EdDsa
    )
}

fn validate_authorization_code(
    value: Zeroizing<String>,
) -> Result<Zeroizing<String>, OidcProviderError> {
    if value.is_empty() || value.len() > MAX_OIDC_CODE_BYTES || value.contains(['\0', '\r', '\n']) {
        return Err(OidcProviderError::Rejected);
    }
    Ok(value)
}

fn map_discovery_error(
    error: openidconnect::DiscoveryError<BoundedOidcHttpError>,
) -> OidcProviderError {
    match error {
        openidconnect::DiscoveryError::Validation(_) => {
            protocol("discovery document issuer did not match the configured issuer")
        }
        openidconnect::DiscoveryError::Parse(_)
        | openidconnect::DiscoveryError::Response(_, _, _)
        | openidconnect::DiscoveryError::UrlParse(_)
        | openidconnect::DiscoveryError::Other(_) => {
            protocol("OIDC discovery response was invalid")
        }
        openidconnect::DiscoveryError::Request(BoundedOidcHttpError::Request(_)) => {
            OidcProviderError::Unavailable
        }
        openidconnect::DiscoveryError::Request(
            BoundedOidcHttpError::UnsafeUrl | BoundedOidcHttpError::ResponseTooLarge,
        ) => protocol("OIDC discovery attempted an unsafe or oversized response"),
        _ => protocol("OIDC discovery response was invalid"),
    }
}

fn map_token_error<Response>(
    error: openidconnect::RequestTokenError<BoundedOidcHttpError, Response>,
) -> OidcProviderError
where
    Response: openidconnect::ErrorResponse + 'static,
{
    match error {
        openidconnect::RequestTokenError::ServerResponse(_) => OidcProviderError::Rejected,
        openidconnect::RequestTokenError::Request(BoundedOidcHttpError::Request(_)) => {
            OidcProviderError::Unavailable
        }
        openidconnect::RequestTokenError::Request(
            BoundedOidcHttpError::UnsafeUrl | BoundedOidcHttpError::ResponseTooLarge,
        ) => protocol("OIDC token exchange attempted an unsafe or oversized response"),
        openidconnect::RequestTokenError::Parse(_, _)
        | openidconnect::RequestTokenError::Other(_) => protocol("OIDC token response was invalid"),
    }
}

fn protocol(message: impl Into<String>) -> OidcProviderError {
    OidcProviderError::Protocol(message.into())
}

#[cfg(test)]
#[path = "openid_connect_provider_tests.rs"]
mod tests;
