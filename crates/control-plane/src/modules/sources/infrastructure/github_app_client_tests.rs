use super::*;
use crate::modules::sources::domain::{
    GithubInstallationId, GithubInstallationVerificationRequest, IGithubAppAuthorizationService,
};
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap as AxumHeaderMap, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

fn client() -> GithubAppClient {
    GithubAppClient::for_test(
        Duration::from_secs(1),
        "a3s-cloud-test",
        "Iv1.test-client",
        "A3S_CLOUD_TEST_GITHUB_CLIENT_SECRET",
        Url::parse("http://127.0.0.1:3000/api/v1/source-connections/github/callback")
            .expect("callback URL"),
        Url::parse("http://github.test/apps/").expect("install URL"),
        Url::parse("http://github.test/login/oauth/authorize").expect("authorization URL"),
        Url::parse("http://github.test/login/oauth/access_token").expect("token URL"),
        Url::parse("http://api.github.test/").expect("API URL"),
    )
    .expect("GitHub App client")
}

#[test]
fn authorization_urls_are_exact_state_and_pkce_bound() {
    let client = client();
    let state = "a".repeat(43);
    let challenge = "b".repeat(43);
    let install = Url::parse(&client.installation_url(&state).expect("installation URL"))
        .expect("parsed installation URL");
    assert_eq!(install.path(), "/apps/a3s-cloud-test/installations/new");
    assert_eq!(
        install
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned()),
        Some(state.clone())
    );

    let authorize = Url::parse(
        &client
            .authorization_url(&state, &challenge)
            .expect("authorization URL"),
    )
    .expect("parsed authorization URL");
    let query = authorize
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query["client_id"], "Iv1.test-client");
    assert_eq!(query["state"], state);
    assert_eq!(query["code_challenge"], challenge);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["redirect_uri"],
        "http://127.0.0.1:3000/api/v1/source-connections/github/callback"
    );
}

#[test]
fn disabled_or_malformed_clients_fail_closed() {
    assert_eq!(
        GithubAppClient::disabled().installation_url(&"a".repeat(43)),
        Err(GithubAppAuthorizationError::NotConfigured)
    );
    assert!(GithubAppClient::for_test(
        Duration::from_secs(1),
        "Bad Slug",
        "client",
        "A3S_CLOUD_TEST_GITHUB_CLIENT_SECRET",
        Url::parse("http://127.0.0.1/api/v1/source-connections/github/callback").expect("callback"),
        Url::parse("http://github.test/apps/").expect("install"),
        Url::parse("http://github.test/authorize").expect("authorize"),
        Url::parse("http://github.test/token").expect("token"),
        Url::parse("http://api.github.test/").expect("API"),
    )
    .is_err());
    assert!(client().installation_url("short").is_err());
}

#[tokio::test]
async fn exchanges_the_oauth_code_and_verifies_the_user_installation_intersection() {
    let capture = Arc::new(RequestCapture::default());
    let (address, server) = start_fixture(
        Router::new()
            .route("/login/oauth/access_token", post(successful_token))
            .route("/user", get(successful_user))
            .route("/user/installations", get(successful_installations))
            .with_state(Arc::clone(&capture)),
    )
    .await;
    let secret = TestEnvironmentVariable::new("fixture-client-secret");
    let client = fixture_client(address, secret.name());

    let verified = client
        .verify_installation(verification_request(42))
        .await
        .expect("verified GitHub installation");

    assert_eq!(verified.installation_id.as_u64(), 42);
    assert_eq!(verified.account_id.as_u64(), 100);
    assert_eq!(verified.account_login.as_str(), "A3S-Lab");
    assert_eq!(verified.account_kind, GithubAccountKind::Organization);
    assert_eq!(verified.user_id.as_u64(), 200);
    assert_eq!(verified.user_login.as_str(), "octocat");

    let token_form = capture
        .token_form
        .lock()
        .expect("token form capture")
        .clone()
        .expect("token form");
    assert_eq!(
        token_form,
        BTreeMap::from([
            ("client_id".into(), "Iv1.test-client".into()),
            ("client_secret".into(), "fixture-client-secret".into()),
            ("code".into(), "fixture-oauth-code".into()),
            (
                "redirect_uri".into(),
                format!("http://{address}/api/v1/source-connections/github/callback"),
            ),
            ("code_verifier".into(), "v".repeat(43)),
        ])
    );
    let token_headers = capture
        .token_headers
        .lock()
        .expect("token header capture")
        .clone()
        .expect("token headers");
    assert_eq!(header(&token_headers, "accept"), "application/json");
    assert_eq!(
        header(&token_headers, "content-type"),
        "application/x-www-form-urlencoded"
    );
    assert_eq!(
        header(&token_headers, "user-agent"),
        "a3s-cloud-control-plane"
    );

    let user_headers = capture
        .user_headers
        .lock()
        .expect("user header capture")
        .clone()
        .expect("user headers");
    assert_api_headers(&user_headers);
    let installation_headers = capture
        .installation_headers
        .lock()
        .expect("installation header capture")
        .clone()
        .expect("installation headers");
    assert_api_headers(&installation_headers);
    assert_eq!(
        capture
            .installation_query
            .lock()
            .expect("installation query capture")
            .clone()
            .expect("installation query"),
        BTreeMap::from([
            ("page".into(), "1".into()),
            ("per_page".into(), "100".into()),
        ])
    );
    server.abort();
}

#[tokio::test]
async fn rejected_oauth_codes_are_classified_without_exposing_secret_values() {
    let (address, server) = start_fixture(Router::new().route(
        "/login/oauth/access_token",
        post(|| async {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "bad_verification_code",
                    "error_description":
                        "fixture-client-secret fixture-oauth-code fixture-pkce-secret"
                })),
            )
        }),
    ))
    .await;
    let secret = TestEnvironmentVariable::new("fixture-client-secret");
    let client = fixture_client(address, secret.name());

    let error = client
        .verify_installation(verification_request(42))
        .await
        .expect_err("rejected OAuth code");

    assert_eq!(error, GithubAppAuthorizationError::Rejected);
    assert_secretless(
        &error,
        &[
            "fixture-client-secret",
            "fixture-oauth-code",
            "fixture-pkce-secret",
        ],
    );
    server.abort();
}

#[tokio::test]
async fn an_installation_outside_the_user_token_intersection_is_forbidden() {
    let (address, server) = start_fixture(
        Router::new()
            .route("/login/oauth/access_token", post(static_token))
            .route("/user", get(static_user))
            .route(
                "/user/installations",
                get(|| async {
                    Json(json!({
                        "total_count": 1,
                        "installations": [{
                            "id": 777,
                            "account": {
                                "id": 700,
                                "login": "unrelated",
                                "type": "Organization"
                            }
                        }]
                    }))
                }),
            ),
    )
    .await;
    let secret = TestEnvironmentVariable::new("fixture-client-secret");
    let client = fixture_client(address, secret.name());

    let error = client
        .verify_installation(verification_request(42))
        .await
        .expect_err("inaccessible installation");

    assert_eq!(error, GithubAppAuthorizationError::Forbidden);
    assert_secretless(&error, &["fixture-client-secret", "fixture-access-token"]);
    server.abort();
}

#[tokio::test]
async fn oversized_and_malformed_provider_responses_fail_closed_and_secretless() {
    let (oversized_address, oversized_server) = start_fixture(Router::new().route(
        "/login/oauth/access_token",
        post(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(format!(
                    "{{\"access_token\":\"fixture-response-secret{}\"}}",
                    "x".repeat(MAX_RESPONSE_BYTES as usize)
                ))
                .expect("oversized response")
        }),
    ))
    .await;
    let oversized_secret = TestEnvironmentVariable::new("fixture-client-secret");
    let oversized_client = fixture_client(oversized_address, oversized_secret.name());

    let oversized_error = oversized_client
        .verify_installation(verification_request(42))
        .await
        .expect_err("oversized token response");
    assert!(matches!(
        &oversized_error,
        GithubAppAuthorizationError::Protocol(message)
            if message == "GitHub response exceeded the size limit"
    ));
    assert_secretless(
        &oversized_error,
        &["fixture-client-secret", "fixture-response-secret"],
    );
    oversized_server.abort();

    let (malformed_address, malformed_server) = start_fixture(
        Router::new()
            .route("/login/oauth/access_token", post(static_token))
            .route(
                "/user",
                get(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body("{\"id\":200,\"login\":\"fixture-user-secret\"".to_owned())
                        .expect("malformed response")
                }),
            ),
    )
    .await;
    let malformed_secret = TestEnvironmentVariable::new("fixture-client-secret");
    let malformed_client = fixture_client(malformed_address, malformed_secret.name());

    let malformed_error = malformed_client
        .verify_installation(verification_request(42))
        .await
        .expect_err("malformed user response");
    assert!(matches!(
        &malformed_error,
        GithubAppAuthorizationError::Protocol(message)
            if message == "GitHub API response JSON is invalid"
    ));
    assert_secretless(
        &malformed_error,
        &[
            "fixture-client-secret",
            "fixture-access-token",
            "fixture-user-secret",
        ],
    );
    malformed_server.abort();
}

#[derive(Default)]
struct RequestCapture {
    token_form: Mutex<Option<BTreeMap<String, String>>>,
    token_headers: Mutex<Option<AxumHeaderMap>>,
    user_headers: Mutex<Option<AxumHeaderMap>>,
    installation_headers: Mutex<Option<AxumHeaderMap>>,
    installation_query: Mutex<Option<BTreeMap<String, String>>>,
}

async fn successful_token(
    State(capture): State<Arc<RequestCapture>>,
    headers: AxumHeaderMap,
    Form(form): Form<BTreeMap<String, String>>,
) -> Json<Value> {
    *capture.token_form.lock().expect("token form capture") = Some(form);
    *capture.token_headers.lock().expect("token header capture") = Some(headers);
    Json(json!({
        "access_token": "fixture-access-token",
        "refresh_token": "fixture-refresh-token",
        "token_type": "bearer"
    }))
}

async fn successful_user(
    State(capture): State<Arc<RequestCapture>>,
    headers: AxumHeaderMap,
) -> Json<Value> {
    *capture.user_headers.lock().expect("user header capture") = Some(headers);
    static_user().await
}

async fn successful_installations(
    State(capture): State<Arc<RequestCapture>>,
    headers: AxumHeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Value> {
    *capture
        .installation_headers
        .lock()
        .expect("installation header capture") = Some(headers);
    *capture
        .installation_query
        .lock()
        .expect("installation query capture") = Some(query);
    Json(json!({
        "total_count": 1,
        "installations": [{
            "id": 42,
            "account": {
                "id": 100,
                "login": "A3S-Lab",
                "type": "Organization"
            }
        }]
    }))
}

async fn static_token() -> Json<Value> {
    Json(json!({
        "access_token": "fixture-access-token",
        "token_type": "bearer"
    }))
}

async fn static_user() -> Json<Value> {
    Json(json!({"id": 200, "login": "octocat"}))
}

fn fixture_client(address: std::net::SocketAddr, client_secret_env: &str) -> GithubAppClient {
    GithubAppClient::for_test(
        Duration::from_secs(2),
        "a3s-cloud-test",
        "Iv1.test-client",
        client_secret_env,
        Url::parse(&format!(
            "http://{address}/api/v1/source-connections/github/callback"
        ))
        .expect("callback URL"),
        Url::parse(&format!("http://{address}/apps/")).expect("install URL"),
        Url::parse(&format!("http://{address}/login/oauth/authorize")).expect("authorization URL"),
        Url::parse(&format!("http://{address}/login/oauth/access_token")).expect("token URL"),
        Url::parse(&format!("http://{address}/")).expect("API URL"),
    )
    .expect("GitHub App client")
}

fn verification_request(installation_id: u64) -> GithubInstallationVerificationRequest {
    GithubInstallationVerificationRequest {
        code: Zeroizing::new("fixture-oauth-code".into()),
        pkce_verifier: Zeroizing::new("v".repeat(43)),
        installation_id: GithubInstallationId::parse(installation_id).expect("installation ID"),
    }
}

async fn start_fixture(router: Router) -> (std::net::SocketAddr, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("GitHub fixture listener");
    let address = listener.local_addr().expect("GitHub fixture address");
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("GitHub fixture server")
    });
    (address, server)
}

fn header<'a>(headers: &'a AxumHeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .expect("fixture request header")
}

fn assert_api_headers(headers: &AxumHeaderMap) {
    assert_eq!(
        header(headers, "authorization"),
        "Bearer fixture-access-token"
    );
    assert_eq!(header(headers, "accept"), "application/vnd.github+json");
    assert_eq!(header(headers, "x-github-api-version"), GITHUB_API_VERSION);
    assert_eq!(header(headers, "user-agent"), "a3s-cloud-control-plane");
}

fn assert_secretless(error: &GithubAppAuthorizationError, secrets: &[&str]) {
    let rendered = format!("{error:?}: {error}");
    for secret in secrets {
        assert!(
            !rendered.contains(secret),
            "provider error exposed a secret value"
        );
    }
}

struct TestEnvironmentVariable {
    name: String,
}

impl TestEnvironmentVariable {
    fn new(value: &str) -> Self {
        let name = format!(
            "A3S_CLOUD_GITHUB_CLIENT_SECRET_TEST_{}",
            Uuid::new_v4().simple().to_string().to_ascii_uppercase()
        );
        std::env::set_var(&name, value);
        Self { name }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for TestEnvironmentVariable {
    fn drop(&mut self) {
        std::env::remove_var(&self.name);
    }
}
