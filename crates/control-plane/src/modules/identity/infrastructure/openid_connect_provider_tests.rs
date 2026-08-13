use super::*;
use crate::modules::shared_kernel::application::OAUTH_FLOW_SECRET_LENGTH;
use axum::body::Body;
use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use openidconnect::core::{
    CoreEdDsaPrivateSigningKey, CoreIdToken, CoreIdTokenClaims, CoreJsonWebKey, CoreJsonWebKeySet,
    CoreJwsSigningAlgorithm,
};
use openidconnect::{
    AccessToken, Audience, ClientId, IssuerUrl, JsonWebKeyId, Nonce, PrivateSigningKey,
    StandardClaims, SubjectIdentifier,
};
use rcgen::{generate_simple_self_signed, CertifiedKey, KeyPair, PKCS_ED25519};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

const CLIENT_ID: &str = "a3s-cloud-oidc-test";
const CLIENT_SECRET: &str = "fixture-client-secret-value";
const SUBJECT: &str = "fixture-subject-42";

#[tokio::test]
async fn discovers_authorizes_and_verifies_rotated_jwks_with_exact_pkce() {
    let nonce = secret('n');
    let fixture = OidcFixture::start(DiscoveryMode::Valid, nonce.clone()).await;
    let environment = TestEnvironmentVariable::new(CLIENT_SECRET);
    let config = fixture.config(environment.name());
    let service = fixture.service(&config);
    let state = secret('s');
    let verifier = secret('v');

    let authorization = service
        .authorization_url(OidcAuthorizationRequest {
            provider_key: provider_key(),
            state: Zeroizing::new(state.clone()),
            nonce: Zeroizing::new(nonce.clone()),
            pkce_verifier: Zeroizing::new(verifier.clone()),
        })
        .await
        .expect("OIDC authorization URL");

    let url = Url::parse(&authorization.authorization_url).expect("authorization URL");
    assert_eq!(url.origin(), fixture.issuer_url().origin());
    assert_eq!(url.path(), "/authorize");
    let query = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
    assert_eq!(query.get("client_id").map(String::as_str), Some(CLIENT_ID));
    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(query.get("scope").map(String::as_str), Some("openid"));
    assert_eq!(query.get("state").map(String::as_str), Some(state.as_str()));
    assert_eq!(query.get("nonce").map(String::as_str), Some(nonce.as_str()));
    assert_eq!(
        query.get("code_challenge").map(String::as_str),
        Some(pkce_s256_challenge(&verifier).as_str())
    );
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert_eq!(
        authorization.provider_config_digest,
        config.public_config_digest().expect("digest")
    );
    assert_eq!(
        authorization.flow_lifetime,
        ChronoDuration::milliseconds(config.flow_ttl_ms as i64)
    );

    fixture.rotate_signing_key();
    let verified = service
        .verify_code(OidcCodeVerificationRequest {
            provider_key: provider_key(),
            code: Zeroizing::new("fixture-code".into()),
            nonce: Zeroizing::new(nonce),
            pkce_verifier: Zeroizing::new(verifier.clone()),
        })
        .await
        .expect("rotated JWKS callback");

    assert_eq!(verified.issuer.as_str(), fixture.issuer());
    assert_eq!(verified.subject.as_str(), SUBJECT);
    assert_eq!(
        verified.login_token_lifetime,
        ChronoDuration::milliseconds(config.login_token_ttl_ms as i64)
    );
    assert_eq!(fixture.state.jwks_requests.load(Ordering::SeqCst), 2);
    let exchange = fixture.state.last_exchange();
    assert_eq!(
        exchange.form.get("grant_type").map(String::as_str),
        Some("authorization_code")
    );
    assert_eq!(
        exchange.form.get("code").map(String::as_str),
        Some("fixture-code")
    );
    assert_eq!(
        exchange.form.get("code_verifier").map(String::as_str),
        Some(verifier.as_str())
    );
    assert_eq!(
        exchange.form.get("redirect_uri").map(String::as_str),
        Some(config.callback_url.as_str())
    );
    assert!(!exchange.form.contains_key("client_secret"));
    assert!(exchange.authorization.starts_with("Basic "));
    assert!(!format!("{verified:?}").contains(CLIENT_SECRET));
}

#[tokio::test]
async fn rejects_wrong_audience_nonce_issuer_signature_and_time_bounds() {
    let nonce = secret('n');
    let fixture = OidcFixture::start(DiscoveryMode::Valid, nonce.clone()).await;
    let environment = TestEnvironmentVariable::new(CLIENT_SECRET);
    let config = fixture.config(environment.name());
    let service = fixture.service(&config);

    fixture.update_claims(|profile| profile.audiences = vec!["another-client".into()]);
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| {
        profile.audiences = vec![CLIENT_ID.into(), "another-client".into()]
    });
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.authorized_party = Some("another-client".into()));
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.nonce = secret('x'));
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.issuer = "https://unknown-issuer.example.test".into());
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.replace_signing_key_without_advertising();
    assert_rejected(&service, &nonce).await;

    fixture.synchronize_advertised_key();
    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.issued_at_offset_seconds = -1800);
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.expires_at_offset_seconds = -1);
    assert_rejected(&service, &nonce).await;

    fixture.reset_claims(&nonce);
    fixture.update_claims(|profile| profile.returned_access_token = "substituted-token".into());
    assert_rejected(&service, &nonce).await;
}

#[tokio::test]
async fn rejects_mismatched_issuer_unsafe_endpoint_redirects_and_unknown_provider() {
    let nonce = secret('n');
    for mode in [
        DiscoveryMode::MismatchedIssuer,
        DiscoveryMode::UnsafeAuthorizationEndpoint,
        DiscoveryMode::Redirect,
        DiscoveryMode::Oversized,
    ] {
        let fixture = OidcFixture::start(mode, nonce.clone()).await;
        let environment = TestEnvironmentVariable::new(CLIENT_SECRET);
        let config = fixture.config(environment.name());
        let service = fixture.service(&config);
        let error = service
            .authorization_url(OidcAuthorizationRequest {
                provider_key: provider_key(),
                state: Zeroizing::new(secret('s')),
                nonce: Zeroizing::new(nonce.clone()),
                pkce_verifier: Zeroizing::new(secret('v')),
            })
            .await
            .expect_err("unsafe discovery must fail closed");
        assert!(matches!(error, OidcProviderError::Protocol(_)));
        assert!(!format!("{error:?}: {error}").contains(CLIENT_SECRET));
        if mode == DiscoveryMode::Redirect {
            assert_eq!(fixture.state.redirect_target_hits.load(Ordering::SeqCst), 0);
        }
    }

    let fixture = OidcFixture::start(DiscoveryMode::Valid, nonce.clone()).await;
    let environment = TestEnvironmentVariable::new(CLIENT_SECRET);
    let config = fixture.config(environment.name());
    let service = fixture.service(&config);
    environment.remove();
    let error = service
        .verify_code(OidcCodeVerificationRequest {
            provider_key: provider_key(),
            code: Zeroizing::new("fixture-code".into()),
            nonce: Zeroizing::new(nonce.clone()),
            pkce_verifier: Zeroizing::new(secret('v')),
        })
        .await
        .expect_err("missing client secret must fail closed");
    assert_eq!(error, OidcProviderError::CredentialUnavailable);
    assert!(fixture
        .state
        .exchanges
        .lock()
        .expect("exchanges")
        .is_empty());

    let service = OpenIdConnectProviderService::new(&[]).expect("empty provider registry");
    let error = service
        .authorization_url(OidcAuthorizationRequest {
            provider_key: provider_key(),
            state: Zeroizing::new(secret('s')),
            nonce: Zeroizing::new(nonce),
            pkce_verifier: Zeroizing::new(secret('v')),
        })
        .await
        .expect_err("unknown provider");
    assert_eq!(error, OidcProviderError::NotConfigured);
}

#[tokio::test]
async fn config_digest_is_canonical_and_never_depends_on_secret_value() {
    let fixture = OidcFixture::start(DiscoveryMode::Valid, secret('n')).await;
    let first_environment = TestEnvironmentVariable::new("first-secret");
    let first = fixture.config(first_environment.name());
    let first_digest = first.public_config_digest().expect("first digest");
    std::env::set_var(first_environment.name(), "rotated-secret");
    assert_eq!(
        first.public_config_digest().expect("rotated digest"),
        first_digest
    );

    let mut changed = first.clone();
    changed.client_id.push_str("-changed");
    assert_ne!(
        changed.public_config_digest().expect("changed digest"),
        first_digest
    );
    assert!(!first_digest.as_str().contains("first-secret"));
    assert!(!first_digest.as_str().contains("rotated-secret"));
}

async fn assert_rejected(service: &OpenIdConnectProviderService, nonce: &str) {
    let error = service
        .verify_code(OidcCodeVerificationRequest {
            provider_key: provider_key(),
            code: Zeroizing::new("fixture-code".into()),
            nonce: Zeroizing::new(nonce.to_owned()),
            pkce_verifier: Zeroizing::new(secret('v')),
        })
        .await
        .expect_err("invalid ID token must be rejected");
    assert_eq!(error, OidcProviderError::Rejected);
    assert!(!format!("{error:?}: {error}").contains(CLIENT_SECRET));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscoveryMode {
    Valid,
    MismatchedIssuer,
    UnsafeAuthorizationEndpoint,
    Redirect,
    Oversized,
}

struct OidcFixture {
    issuer: String,
    root: reqwest::Certificate,
    state: Arc<FixtureState>,
    server: JoinHandle<()>,
}

struct FixtureState {
    issuer: String,
    mode: DiscoveryMode,
    advertised_key: RwLock<CoreJsonWebKey>,
    signing_key: RwLock<Arc<CoreEdDsaPrivateSigningKey>>,
    claims: Mutex<ClaimsProfile>,
    exchanges: Mutex<Vec<TokenExchange>>,
    jwks_requests: AtomicUsize,
    redirect_target_hits: AtomicUsize,
}

#[derive(Clone)]
struct ClaimsProfile {
    issuer: String,
    audiences: Vec<String>,
    authorized_party: Option<String>,
    nonce: String,
    subject: String,
    signed_access_token: String,
    returned_access_token: String,
    issued_at_offset_seconds: i64,
    expires_at_offset_seconds: i64,
}

#[derive(Clone)]
struct TokenExchange {
    authorization: String,
    form: HashMap<String, String>,
}

impl OidcFixture {
    async fn start(mode: DiscoveryMode, nonce: String) -> Self {
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(vec!["127.0.0.1".into()]).expect("fixture TLS");
        let root = reqwest::Certificate::from_der(cert.der().as_ref()).expect("fixture root");
        let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("fixture TLS protocol versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
        )
        .expect("fixture server TLS");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let issuer = format!("https://{address}");
        let signing_key = Arc::new(new_signing_key("fixture-key-1"));
        let state = Arc::new(FixtureState {
            issuer: issuer.clone(),
            mode,
            advertised_key: RwLock::new(signing_key.as_verification_key()),
            signing_key: RwLock::new(signing_key),
            claims: Mutex::new(ClaimsProfile::valid(&issuer, nonce)),
            exchanges: Mutex::new(Vec::new()),
            jwks_requests: AtomicUsize::new(0),
            redirect_target_hits: AtomicUsize::new(0),
        });
        let router = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/redirected-discovery", get(redirected_discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .route("/authorize", get(|| async { StatusCode::NO_CONTENT }))
            .with_state(Arc::clone(&state));
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        let server = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let service = TowerToHyperService::new(router);
                    let io = TokioIo::new(tls);
                    let builder = ConnectionBuilder::new(TokioExecutor::new());
                    let _ = builder.serve_connection(io, service).await;
                });
            }
        });
        Self {
            issuer,
            root,
            state,
            server,
        }
    }

    fn issuer(&self) -> &str {
        &self.issuer
    }

    fn issuer_url(&self) -> Url {
        Url::parse(&self.issuer).expect("fixture issuer")
    }

    fn config(&self, client_secret_env: &str) -> OidcProviderConfig {
        OidcProviderConfig {
            key: "workforce".into(),
            issuer: self.issuer.clone(),
            client_id: CLIENT_ID.into(),
            client_secret_env: client_secret_env.into(),
            callback_url: "https://cloud.example.test/api/v1/identity/oidc/workforce/callback"
                .into(),
            request_timeout_ms: 5_000,
            flow_ttl_ms: 300_000,
            login_token_ttl_ms: 3_600_000,
        }
    }

    fn service(&self, config: &OidcProviderConfig) -> OpenIdConnectProviderService {
        let provider = Provider::for_test(config, self.root.clone()).expect("fixture provider");
        OpenIdConnectProviderService {
            providers: Arc::new(BTreeMap::from([(provider.key.clone(), provider)])),
        }
    }

    fn rotate_signing_key(&self) {
        let key = Arc::new(new_signing_key("fixture-key-2"));
        *self.state.signing_key.write().expect("signing key") = Arc::clone(&key);
        *self.state.advertised_key.write().expect("advertised key") = key.as_verification_key();
    }

    fn replace_signing_key_without_advertising(&self) {
        *self.state.signing_key.write().expect("signing key") =
            Arc::new(new_signing_key("unadvertised-key"));
    }

    fn synchronize_advertised_key(&self) {
        let key = self.state.signing_key.read().expect("signing key");
        *self.state.advertised_key.write().expect("advertised key") = key.as_verification_key();
    }

    fn reset_claims(&self, nonce: &str) {
        *self.state.claims.lock().expect("claims") =
            ClaimsProfile::valid(&self.issuer, nonce.to_owned());
    }

    fn update_claims(&self, update: impl FnOnce(&mut ClaimsProfile)) {
        update(&mut self.state.claims.lock().expect("claims"));
    }
}

impl Drop for OidcFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl ClaimsProfile {
    fn valid(issuer: &str, nonce: String) -> Self {
        Self {
            issuer: issuer.into(),
            audiences: vec![CLIENT_ID.into()],
            authorized_party: None,
            nonce,
            subject: SUBJECT.into(),
            signed_access_token: "discarded-provider-access-token".into(),
            returned_access_token: "discarded-provider-access-token".into(),
            issued_at_offset_seconds: 0,
            expires_at_offset_seconds: 300,
        }
    }
}

impl FixtureState {
    fn last_exchange(&self) -> TokenExchange {
        self.exchanges
            .lock()
            .expect("exchanges")
            .last()
            .expect("token exchange")
            .clone()
    }
}

async fn discovery(State(state): State<Arc<FixtureState>>) -> Response {
    if state.mode == DiscoveryMode::Redirect {
        return Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, "/redirected-discovery")
            .body(Body::empty())
            .expect("redirect response");
    }
    if state.mode == DiscoveryMode::Oversized {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(vec![b'x'; MAX_OIDC_RESPONSE_BYTES as usize + 1]))
            .expect("oversized response");
    }
    discovery_json(&state, state.mode).into_response()
}

async fn redirected_discovery(State(state): State<Arc<FixtureState>>) -> Response {
    state.redirect_target_hits.fetch_add(1, Ordering::SeqCst);
    discovery_json(&state, DiscoveryMode::Valid).into_response()
}

fn discovery_json(state: &FixtureState, mode: DiscoveryMode) -> Json<Value> {
    let issuer = if mode == DiscoveryMode::MismatchedIssuer {
        "https://unknown-issuer.example.test".to_owned()
    } else {
        state.issuer.clone()
    };
    let authorization_endpoint = if mode == DiscoveryMode::UnsafeAuthorizationEndpoint {
        "http://127.0.0.1/authorize".to_owned()
    } else {
        format!("{}/authorize", state.issuer)
    };
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": authorization_endpoint,
        "token_endpoint": format!("{}/token", state.issuer),
        "jwks_uri": format!("{}/jwks", state.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA", "none", "HS256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"]
    }))
}

async fn jwks(State(state): State<Arc<FixtureState>>) -> Json<Value> {
    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
    let key = state.advertised_key.read().expect("advertised key").clone();
    Json(serde_json::to_value(CoreJsonWebKeySet::new(vec![key])).expect("JWKS JSON"))
}

async fn token(
    State(state): State<Arc<FixtureState>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    state
        .exchanges
        .lock()
        .expect("exchanges")
        .push(TokenExchange {
            authorization,
            form,
        });
    let ClaimsProfile {
        issuer,
        audiences,
        authorized_party,
        nonce,
        subject,
        signed_access_token,
        returned_access_token,
        issued_at_offset_seconds,
        expires_at_offset_seconds,
    } = state.claims.lock().expect("claims").clone();
    let signing_key = state.signing_key.read().expect("signing key").clone();
    let now = Utc::now();
    let claims = CoreIdTokenClaims::new(
        IssuerUrl::new(issuer).expect("claim issuer"),
        audiences.into_iter().map(Audience::new).collect(),
        now + ChronoDuration::seconds(expires_at_offset_seconds),
        now + ChronoDuration::seconds(issued_at_offset_seconds),
        StandardClaims::new(SubjectIdentifier::new(subject)),
        Default::default(),
    )
    .set_nonce(Some(Nonce::new(nonce)))
    .set_authorized_party(authorized_party.map(ClientId::new));
    let signed_access_token = AccessToken::new(signed_access_token);
    let id_token = CoreIdToken::new(
        claims,
        signing_key.as_ref(),
        CoreJwsSigningAlgorithm::EdDsa,
        Some(&signed_access_token),
        None,
    )
    .expect("fixture ID token");
    Json(json!({
        "access_token": returned_access_token,
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token.to_string()
    }))
}

fn new_signing_key(kid: &str) -> CoreEdDsaPrivateSigningKey {
    let key = KeyPair::generate_for(&PKCS_ED25519).expect("Ed25519 fixture key");
    CoreEdDsaPrivateSigningKey::from_ed25519_pem(
        &key.serialize_pem(),
        Some(JsonWebKeyId::new(kid.into())),
    )
    .expect("OIDC Ed25519 key")
}

fn provider_key() -> OidcProviderKey {
    OidcProviderKey::parse("workforce").expect("provider key")
}

fn secret(character: char) -> String {
    character.to_string().repeat(OAUTH_FLOW_SECRET_LENGTH)
}

struct TestEnvironmentVariable {
    name: String,
}

impl TestEnvironmentVariable {
    fn new(value: &str) -> Self {
        let name = format!(
            "A3S_CLOUD_OIDC_SECRET_TEST_{}",
            Uuid::new_v4().simple().to_string().to_ascii_uppercase()
        );
        std::env::set_var(&name, value);
        Self { name }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn remove(&self) {
        std::env::remove_var(&self.name);
    }
}

impl Drop for TestEnvironmentVariable {
    fn drop(&mut self) {
        std::env::remove_var(&self.name);
    }
}
