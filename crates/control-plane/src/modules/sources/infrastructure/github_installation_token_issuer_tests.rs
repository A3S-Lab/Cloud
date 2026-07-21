use super::*;
use crate::modules::shared_kernel::domain::{OrganizationId, SourceConnectionId};
use crate::modules::sources::domain::{
    GitProvider, GithubInstallationAuthorityError, GithubInstallationAuthorityRequest,
    GithubInstallationId, GithubInstallationTokenRequest, GithubProviderAuthorityState,
    IGithubInstallationAuthorityProvider, IGithubInstallationTokenService,
};
use axum::extract::State;
use axum::http::HeaderMap as AxumHeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCsKzLhKHhpiENt
5BBMeTOVucup3v9Eh8ACMwSJxZmnH9oMVsxxl1ukrABKmmb88zmN7I5lFwNl4Y7k
vlexAHAqcxd/S4IkVjJktgmlfbrP73wmtOmUjMgxkby6Oum7YLqhgJlztltXfbMS
bVpYLW+PMmepsCY9fnM3QUiX7hAPC2+pXUGR27JRLx28jzSbyabty6btjrCpd6Zo
JYZbOQ2I2M6kc6Ze5q5PBZi3EK+2PdOHoPG0DIp26gpQtW4wT7QV8mg3Wb6lBgXt
OsPW0OCxsb2JaA/4qIGovShsnRodz+7paYvtB9X408FM3MhWYRobz2063N2mOWHI
OgS+groDAgMBAAECggEAFFP9a+xVW1CFdaIp8n2VA6auT15PEY5ds2TGsmWsYLk4
C6Dr/rV6UpUka6qTYmZtcz5tCa6P7iWVs5htzi5ZEAoKyjLmKNgslwuPVATmW8rR
A9j0g1+j+4ZTnEF8e21OW+dNNwZe/pOO4ywaMLMcqvGun7B2s2gMvodsxNOM/dmQ
Ihsfybr4UsC0m6xUl6soZgh0PFa6gi6dMcYIUPIe+iMBM0O9N1umtbo//nNZDJFk
jDKvPvw6nww5vMlFz2Wu7PRTKk7Ibiqvib7xkf/XQiWCr5mizWvX11zapS37NEew
/otp5YRCrRsufmLpY5DvDD45TwVgcBPmiJ/6MXVgEQKBgQDg2j15zwk4khgWYWlQ
5zcXE7c4BqDJtaml+MlHIvvn07uTv0wuqjdv8KjvzR60lbTbFp6Gk7eL2DZwpQ/c
KUUx4QN61BDjTqbHmgPGT/YILLMp64gbbKc4vKCzG8q0yQGYim+RNCqUxiy1DMs2
5unXmsMfttJazDxLbfLgr5HNTwKBgQDEBK2Dcs0dQRLykSzk9mq6KMpRO6hEN7iV
kAGYYWFX2n/GTM6F9ilA1ltpb8GyFOACYUmAC8V90RfAR03hJBroSAOiM4OFwIh5
qZzo+9uIMXdetmmAyTAnzIbJz0/30PjGQGTTDq15yqSL94DNXevCbpyPxh6C74xA
na9klv6jDQKBgQC25/DAKHFIyla7xevEuwDuTcRp18Jtss/YyiL5MfUWZP8eNavD
/gTwkyTpRMMohOtEmQbFVF4nbO8D/NGE9zFpXK/W97DxJua2UYumgx8REUOA4y6p
mF4C4jYa3I2tOCGLM3mD9Zp5wSdW85xPAdHQ5/y3zKEa6S5W8Y5zxX3mMwKBgBUZ
58/huODwU2jXZfzT5hNaNsVd5bRKR02abgGIYiFB/UVMmWLkZ/Z53OdRx4kzJBY9
gNsO6Vis/KCPTHvzFg5xSirY3sy0ODzYnHKcQjq8EHyaqGrbvZpbMxtgfNxRm6ZD
4layGsykmugSYcQ52xpYK/RyQHCZ9wAxuWLbule1AoGBAIAh09yNKrKxqb7fdgKU
4tzPQPknFUfiw1ocOqUtfncQ3grP3KTBL/q+qpWYOceeuUqsjg2IXU5OGxqF/ugR
+kF1CBKoGjBpZQ+0s/4tYgrFeJPhQMeVqRX39e/rP1DkWOb7jqYbCY2N61rF8HqA
JKtcvLZwSNU3w7wh1oiBR/Nh
-----END PRIVATE KEY-----"#;
const FIXTURE_TOKEN: &str = "fixture-installation-token-private";

#[tokio::test]
async fn issues_one_repository_scoped_read_only_token_with_a_bounded_app_jwt() {
    let capture = Arc::new(RequestCapture::default());
    let (address, server) = start_fixture(
        Router::new()
            .route("/app/installations/42/access_tokens", post(issue_token))
            .with_state(Arc::clone(&capture)),
    )
    .await;
    let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
    let issuer = fixture_issuer(address, private_key.name());
    let requested_at = DateTime::parse_from_rfc3339("2026-07-21T00:00:00Z")
        .expect("request time")
        .with_timezone(&Utc);
    let repository = repository();

    let credential = issuer
        .issue(GithubInstallationTokenRequest {
            organization_id: OrganizationId::new(),
            connection_id: SourceConnectionId::new(),
            installation_id: GithubInstallationId::parse(42).expect("installation ID"),
            repository: repository.clone(),
            requested_at,
        })
        .await
        .expect("installation token");

    assert!(credential.authorizes(&repository, requested_at, ChronoDuration::minutes(5)));
    assert!(!format!("{credential:?}").contains(FIXTURE_TOKEN));
    let headers = capture
        .headers
        .lock()
        .expect("header capture")
        .clone()
        .expect("headers");
    assert_eq!(header(&headers, "accept"), "application/vnd.github+json");
    assert_eq!(header(&headers, "x-github-api-version"), GITHUB_API_VERSION);
    assert_eq!(header(&headers, "user-agent"), "a3s-cloud-control-plane");
    let jwt = header(&headers, "authorization")
        .strip_prefix("Bearer ")
        .expect("Bearer app JWT");
    let segments = jwt.split('.').collect::<Vec<_>>();
    assert_eq!(segments.len(), 3);
    let jwt_header: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(segments[0])
            .expect("JWT header encoding"),
    )
    .expect("JWT header");
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(segments[1])
            .expect("JWT claims encoding"),
    )
    .expect("JWT claims");
    assert_eq!(jwt_header, json!({"alg": "RS256", "typ": "JWT"}));
    assert_eq!(claims["iss"], "Iv1.test-client");
    assert_eq!(claims["iat"], requested_at.timestamp() - 60);
    assert_eq!(claims["exp"], requested_at.timestamp() + 540);
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(segments[2])
            .expect("JWT signature")
            .len(),
        256
    );
    assert_eq!(
        capture
            .body
            .lock()
            .expect("body capture")
            .clone()
            .expect("request body"),
        json!({
            "repositories": ["private-cloud"],
            "permissions": {"contents": "read"}
        })
    );
    server.abort();
}

#[tokio::test]
async fn missing_keys_rejected_scope_and_provider_bodies_are_secretless() {
    let missing = GithubInstallationTokenIssuer::for_test(
        Duration::from_secs(1),
        "Iv1.test-client",
        "A3S_CLOUD_MISSING_GITHUB_APP_PRIVATE_KEY",
        Url::parse("http://127.0.0.1/").expect("API URL"),
    )
    .expect("issuer");
    assert_eq!(
        missing
            .issue(token_request())
            .await
            .expect_err("missing key"),
        GithubInstallationTokenError::Unavailable
    );
    assert_eq!(
        GithubInstallationTokenIssuer::disabled()
            .issue(token_request())
            .await
            .expect_err("disabled issuer"),
        GithubInstallationTokenError::NotConfigured
    );

    let secret_body = "fixture-provider-private-key fixture-provider-token";
    let (address, server) = start_fixture(Router::new().route(
        "/app/installations/42/access_tokens",
        post(move || async move { (StatusCode::FORBIDDEN, secret_body) }),
    ))
    .await;
    let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
    let error = fixture_issuer(address, private_key.name())
        .issue(token_request())
        .await
        .expect_err("forbidden repository");
    assert_eq!(error, GithubInstallationTokenError::Forbidden);
    let rendered = format!("{error:?}: {error}");
    assert!(!rendered.contains("fixture-provider-private-key"));
    assert!(!rendered.contains("fixture-provider-token"));
    server.abort();
}

#[tokio::test]
async fn provider_cannot_broaden_permission_scope() {
    let broadened_token = "fixture-broadened-installation-token";
    let (address, server) = start_fixture(Router::new().route(
        "/app/installations/42/access_tokens",
        post(move || async move {
            (
                StatusCode::CREATED,
                Json(json!({
                    "token": broadened_token,
                    "expires_at": "2026-07-21T01:00:00Z",
                    "permissions": {"contents": "read", "issues": "write"},
                    "repository_selection": "selected"
                })),
            )
        }),
    ))
    .await;
    let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
    let error = fixture_issuer(address, private_key.name())
        .issue(token_request())
        .await
        .expect_err("broadened provider scope");

    assert!(matches!(error, GithubInstallationTokenError::Protocol(_)));
    assert!(!format!("{error:?}: {error}").contains(broadened_token));
    server.abort();
}

#[tokio::test]
async fn inspects_authoritative_active_and_suspended_installations() {
    for (suspended_at, expected_state) in [
        (None, GithubProviderAuthorityState::Active),
        (
            Some("2026-07-21T00:00:00Z"),
            GithubProviderAuthorityState::Suspended,
        ),
    ] {
        let (address, server) = start_fixture(Router::new().route(
            "/app/installations/42",
            get(move || async move {
                Json(json!({
                    "id": 42,
                    "account": {
                        "id": 100,
                        "login": "A3S-Lab",
                        "type": "Organization"
                    },
                    "suspended_at": suspended_at
                }))
            }),
        ))
        .await;
        let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
        let authority = fixture_issuer(address, private_key.name())
            .inspect(authority_request())
            .await
            .expect("installation authority");

        assert_eq!(authority.installation_id.as_u64(), 42);
        assert_eq!(authority.state, expected_state);
        let account = authority.account.expect("installation account");
        assert_eq!(account.id.as_u64(), 100);
        assert_eq!(account.login.as_str(), "A3S-Lab");
        server.abort();
    }
}

#[tokio::test]
async fn authoritative_not_found_is_deleted_while_provider_failures_stay_retryable() {
    let (deleted_address, deleted_server) = start_fixture(Router::new().route(
        "/app/installations/42",
        get(|| async { StatusCode::NOT_FOUND }),
    ))
    .await;
    let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
    let deleted = fixture_issuer(deleted_address, private_key.name())
        .inspect(authority_request())
        .await
        .expect("deleted installation authority");
    assert_eq!(deleted.state, GithubProviderAuthorityState::Deleted);
    assert!(deleted.account.is_none());
    deleted_server.abort();

    let (unavailable_address, unavailable_server) = start_fixture(Router::new().route(
        "/app/installations/42",
        get(|| async { StatusCode::TOO_MANY_REQUESTS }),
    ))
    .await;
    assert_eq!(
        fixture_issuer(unavailable_address, private_key.name())
            .inspect(authority_request())
            .await,
        Err(GithubInstallationAuthorityError::Unavailable)
    );
    unavailable_server.abort();

    assert_eq!(
        GithubInstallationTokenIssuer::disabled()
            .inspect(authority_request())
            .await,
        Err(GithubInstallationAuthorityError::NotConfigured)
    );
}

#[tokio::test]
async fn authority_rejects_provider_identity_confusion() {
    let (address, server) = start_fixture(Router::new().route(
        "/app/installations/42",
        get(|| async {
            Json(json!({
                "id": 43,
                "account": {
                    "id": 100,
                    "login": "A3S-Lab",
                    "type": "Organization"
                },
                "suspended_at": null
            }))
        }),
    ))
    .await;
    let private_key = TestEnvironmentVariable::new(TEST_PRIVATE_KEY);
    assert!(matches!(
        fixture_issuer(address, private_key.name())
            .inspect(authority_request())
            .await,
        Err(GithubInstallationAuthorityError::Protocol(_))
    ));
    server.abort();
}

#[derive(Default)]
struct RequestCapture {
    headers: Mutex<Option<AxumHeaderMap>>,
    body: Mutex<Option<Value>>,
}

async fn issue_token(
    State(capture): State<Arc<RequestCapture>>,
    headers: AxumHeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    *capture.headers.lock().expect("header capture") = Some(headers);
    *capture.body.lock().expect("body capture") = Some(body);
    (
        StatusCode::CREATED,
        Json(json!({
            "token": FIXTURE_TOKEN,
            "expires_at": "2026-07-21T01:00:00Z",
            "permissions": {"contents": "read", "metadata": "read"},
            "repository_selection": "selected"
        })),
    )
}

fn fixture_issuer(
    address: std::net::SocketAddr,
    private_key_env: &str,
) -> GithubInstallationTokenIssuer {
    GithubInstallationTokenIssuer::for_test(
        Duration::from_secs(2),
        "Iv1.test-client",
        private_key_env,
        Url::parse(&format!("http://{address}/")).expect("API URL"),
    )
    .expect("installation-token issuer")
}

fn token_request() -> GithubInstallationTokenRequest {
    GithubInstallationTokenRequest {
        organization_id: OrganizationId::new(),
        connection_id: SourceConnectionId::new(),
        installation_id: GithubInstallationId::parse(42).expect("installation ID"),
        repository: repository(),
        requested_at: DateTime::parse_from_rfc3339("2026-07-21T00:00:00Z")
            .expect("request time")
            .with_timezone(&Utc),
    }
}

fn authority_request() -> GithubInstallationAuthorityRequest {
    GithubInstallationAuthorityRequest {
        installation_id: GithubInstallationId::parse(42).expect("installation ID"),
        checked_at: DateTime::parse_from_rfc3339("2026-07-21T00:00:00Z")
            .expect("check time")
            .with_timezone(&Utc),
    }
}

fn repository() -> crate::modules::sources::domain::GitRepository {
    crate::modules::sources::domain::GitRepository::parse(
        GitProvider::Github,
        "https://github.com/a3s-lab/private-cloud",
    )
    .expect("repository")
}

async fn start_fixture(router: Router) -> (std::net::SocketAddr, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener");
    let address = listener.local_addr().expect("fixture address");
    let server =
        tokio::spawn(async move { axum::serve(listener, router).await.expect("fixture server") });
    (address, server)
}

fn header<'a>(headers: &'a AxumHeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .expect("fixture request header")
}

struct TestEnvironmentVariable {
    name: String,
}

impl TestEnvironmentVariable {
    fn new(value: &str) -> Self {
        let name = format!(
            "A3S_CLOUD_GITHUB_PRIVATE_KEY_TEST_{}",
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
