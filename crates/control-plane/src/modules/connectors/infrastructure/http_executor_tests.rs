use super::*;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode as AxumStatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use tokio::process::Command;
use tokio::sync::Mutex;
use uuid::Uuid;

const SYSTEM_PROXY_CHILD_MARKER: &str = "A3S_CONNECTOR_PROXY_CHILD";
const SYSTEM_PROXY_CHILD_VALUE: &str = "a3s-c4-isolated-v1";
const SYSTEM_PROXY_CHILD_TEST: &str =
    "modules::connectors::infrastructure::http_executor::tests::system_proxy_child";

#[derive(Debug, Clone)]
struct CapturedRequest {
    headers: AxumHeaderMap,
    body: Vec<u8>,
}

#[derive(Clone)]
struct FixtureState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    response_status: Arc<AtomicU16>,
    retry_after: Arc<Mutex<Option<String>>>,
    response_body: Arc<Mutex<Vec<u8>>>,
}

struct HttpFixture {
    endpoint: Url,
    address: SocketAddr,
    state: FixtureState,
    server: tokio::task::JoinHandle<()>,
}

impl HttpFixture {
    async fn start(
        status: AxumStatusCode,
        retry_after: Option<&str>,
        response_body: Vec<u8>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let state = FixtureState {
            requests: Arc::new(Mutex::new(Vec::new())),
            response_status: Arc::new(AtomicU16::new(status.as_u16())),
            retry_after: Arc::new(Mutex::new(retry_after.map(str::to_owned))),
            response_body: Arc::new(Mutex::new(response_body)),
        };
        let router = Router::new()
            .route("/delivery/top-secret-endpoint", post(capture_request))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve Connector fixture");
        });
        Self {
            endpoint: Url::parse(&format!("http://{address}/delivery/top-secret-endpoint"))
                .expect("fixture endpoint"),
            address,
            state,
            server,
        }
    }

    async fn requests(&self) -> Vec<CapturedRequest> {
        self.state.requests.lock().await.clone()
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn capture_request(
    State(state): State<FixtureState>,
    headers: AxumHeaderMap,
    body: Bytes,
) -> Response {
    state.requests.lock().await.push(CapturedRequest {
        headers,
        body: body.to_vec(),
    });
    let status = AxumStatusCode::from_u16(state.response_status.load(Ordering::SeqCst))
        .unwrap_or(AxumStatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, state.response_body.lock().await.clone()).into_response();
    if let Some(retry_after) = state.retry_after.lock().await.as_deref() {
        response.headers_mut().insert(
            RETRY_AFTER,
            retry_after.parse().expect("fixture retry-after header"),
        );
    }
    response
}

struct ExactEgressAuthorizer {
    revision_id: ConnectorRevisionId,
    endpoint: Url,
    authorized: Option<AuthorizedConnectorDestination>,
    calls: AtomicUsize,
}

impl ExactEgressAuthorizer {
    fn allowing(revision_id: ConnectorRevisionId, endpoint: Url, address: SocketAddr) -> Self {
        let authorized = AuthorizedConnectorDestination::new(&endpoint, vec![address])
            .expect("authorized fixture destination");
        Self {
            revision_id,
            endpoint,
            authorized: Some(authorized),
            calls: AtomicUsize::new(0),
        }
    }

    fn returning(
        revision_id: ConnectorRevisionId,
        endpoint: Url,
        authorized_endpoint: Url,
        address: SocketAddr,
    ) -> Self {
        let authorized = AuthorizedConnectorDestination::new(&authorized_endpoint, vec![address])
            .expect("authorized fixture destination");
        Self {
            revision_id,
            endpoint,
            authorized: Some(authorized),
            calls: AtomicUsize::new(0),
        }
    }

    fn denying(revision_id: ConnectorRevisionId, endpoint: Url) -> Self {
        Self {
            revision_id,
            endpoint,
            authorized: None,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl IConnectorEgressAuthorizer for ExactEgressAuthorizer {
    async fn authorize(
        &self,
        connector_revision_id: ConnectorRevisionId,
        endpoint: &Url,
    ) -> Result<AuthorizedConnectorDestination, ConnectorExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if connector_revision_id == self.revision_id && endpoint == &self.endpoint {
            if let Some(authorized) = &self.authorized {
                return Ok(authorized.clone());
            }
        }
        Err(ConnectorExecutionError::Rejected)
    }
}

fn resolved_revision(
    revision_id: ConnectorRevisionId,
    endpoint: Url,
    authentication: ResolvedConnectorAuthentication,
    maximum_response_bytes: usize,
) -> ResolvedConnectorHttpRevision {
    ResolvedConnectorHttpRevision::for_test(
        revision_id,
        endpoint,
        ConnectorHttpMethod::Post,
        "application/json",
        16 * 1024,
        maximum_response_bytes,
        Duration::from_secs(2),
        ConnectorHttpStatusPolicy::standard_webhook(),
        authentication,
    )
    .expect("resolved Connector revision")
}

fn request(revision_id: ConnectorRevisionId, body: &[u8]) -> ConnectorExecutionRequest {
    ConnectorExecutionRequest::new(
        revision_id,
        Uuid::now_v7(),
        "application/json",
        body.to_vec(),
    )
    .expect("Connector request")
}

#[tokio::test]
async fn exact_egress_addresses_are_pinned_and_hmac_is_enforced_for_one_attempt() {
    let fixture = HttpFixture::start(AxumStatusCode::OK, None, b"accepted".to_vec()).await;
    let revision_id = ConnectorRevisionId::new();
    let secret = b"0123456789abcdef0123456789abcdef";
    let endpoint = Url::parse(&format!(
        "http://connector.invalid:{}/delivery/top-secret-endpoint",
        fixture.address.port()
    ))
    .expect("pinned fixture endpoint");
    let revision = resolved_revision(
        revision_id,
        endpoint.clone(),
        ResolvedConnectorAuthentication::hmac_sha256(
            Zeroizing::new(secret.to_vec()),
            "x-a3s-signature",
            "v1=",
        )
        .expect("HMAC authentication"),
        1024,
    );
    let debug = format!("{revision:?}");
    assert!(!debug.contains("top-secret-endpoint"));
    assert!(!debug.contains("0123456789abcdef"));
    let egress = Arc::new(ExactEgressAuthorizer::allowing(
        revision_id,
        endpoint,
        fixture.address,
    ));
    let executor = BoundedHttpConnectorExecutor::new(revision, egress.clone());
    let signing_input = b"v1\n2026-08-14T00:00:00Z\nattempt\nbody";
    let request = request(revision_id, br#"{"delivery":"body"}"#)
        .with_header("x-a3s-delivery-id", Uuid::now_v7().to_string())
        .expect("delivery header")
        .with_signing_input(signing_input.to_vec())
        .expect("signing input");
    let receipt = executor.execute(&request).await.expect("Connector attempt");
    assert_eq!(receipt.connector_revision_id(), revision_id);
    assert_eq!(receipt.attempt_id(), request.attempt_id());
    assert_eq!(receipt.response_body(), b"accepted");
    assert_eq!(egress.calls.load(Ordering::SeqCst), 1);

    let requests = fixture.requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body, request.body());
    assert!(requests[0]
        .headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host.starts_with("connector.invalid:")));
    let signature = requests[0]
        .headers
        .get("x-a3s-signature")
        .and_then(|value| value.to_str().ok())
        .expect("signature header");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC");
    mac.update(signing_input);
    let expected = BASE64_STANDARD.encode(mac.finalize().into_bytes());
    assert_eq!(signature, format!("v1={expected}"));
}

#[tokio::test]
async fn egress_denial_prevents_network_access() {
    let fixture = HttpFixture::start(AxumStatusCode::OK, None, Vec::new()).await;
    let revision_id = ConnectorRevisionId::new();
    let egress = Arc::new(ExactEgressAuthorizer::denying(
        revision_id,
        fixture.endpoint.clone(),
    ));
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            fixture.endpoint.clone(),
            ResolvedConnectorAuthentication::none(),
            1024,
        ),
        egress.clone(),
    );
    assert_eq!(
        executor.execute(&request(revision_id, b"body")).await,
        Err(ConnectorExecutionError::Rejected)
    );
    assert_eq!(egress.calls.load(Ordering::SeqCst), 1);
    assert!(fixture.requests().await.is_empty());
}

#[tokio::test]
async fn executor_rejects_an_authorization_bound_to_another_endpoint() {
    let fixture = HttpFixture::start(AxumStatusCode::OK, None, Vec::new()).await;
    let revision_id = ConnectorRevisionId::new();
    let endpoint = Url::parse(&format!(
        "http://connector.invalid:{}/delivery/top-secret-endpoint",
        fixture.address.port()
    ))
    .expect("requested endpoint");
    let other_endpoint = Url::parse(&format!(
        "http://other.invalid:{}/delivery/top-secret-endpoint",
        fixture.address.port()
    ))
    .expect("other endpoint");
    let egress = Arc::new(ExactEgressAuthorizer::returning(
        revision_id,
        endpoint.clone(),
        other_endpoint,
        fixture.address,
    ));
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            endpoint,
            ResolvedConnectorAuthentication::none(),
            1024,
        ),
        egress.clone(),
    );

    assert_eq!(
        executor.execute(&request(revision_id, b"body")).await,
        Err(ConnectorExecutionError::Rejected)
    );
    assert_eq!(egress.calls.load(Ordering::SeqCst), 1);
    assert!(fixture.requests().await.is_empty());
}

#[tokio::test]
async fn system_proxy_is_disabled_for_pinned_connector_attempts() {
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .arg("--exact")
        .arg(SYSTEM_PROXY_CHILD_TEST)
        .arg("--nocapture")
        .env(SYSTEM_PROXY_CHILD_MARKER, SYSTEM_PROXY_CHILD_VALUE)
        .output()
        .await
        .expect("isolated proxy test process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "isolated proxy test failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[tokio::test]
async fn system_proxy_child() {
    if std::env::var(SYSTEM_PROXY_CHILD_MARKER).as_deref() != Ok(SYSTEM_PROXY_CHILD_VALUE) {
        return;
    }

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy trap listener");
    let proxy_url = format!(
        "http://{}",
        proxy_listener.local_addr().expect("proxy trap address")
    );
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        std::env::set_var(name, &proxy_url);
    }
    for name in ["NO_PROXY", "no_proxy"] {
        std::env::remove_var(name);
    }

    let fixture = HttpFixture::start(AxumStatusCode::OK, None, b"direct".to_vec()).await;
    let revision_id = ConnectorRevisionId::new();
    let endpoint = Url::parse(&format!(
        "http://connector.invalid:{}/delivery/top-secret-endpoint",
        fixture.address.port()
    ))
    .expect("pinned fixture endpoint");
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            endpoint.clone(),
            ResolvedConnectorAuthentication::none(),
            1024,
        ),
        Arc::new(ExactEgressAuthorizer::allowing(
            revision_id,
            endpoint,
            fixture.address,
        )),
    );

    let receipt = executor
        .execute(&request(revision_id, b"body"))
        .await
        .expect("direct pinned attempt");
    assert_eq!(receipt.response_body(), b"direct");
    assert_eq!(fixture.requests().await.len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), proxy_listener.accept())
            .await
            .is_err(),
        "Connector attempt must not contact the system proxy"
    );
}

#[tokio::test]
async fn status_retry_after_redirect_and_response_bounds_are_connector_owned() {
    let retrying =
        HttpFixture::start(AxumStatusCode::TOO_MANY_REQUESTS, Some("7"), Vec::new()).await;
    let revision_id = ConnectorRevisionId::new();
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            retrying.endpoint.clone(),
            ResolvedConnectorAuthentication::none(),
            1024,
        ),
        Arc::new(ExactEgressAuthorizer::allowing(
            revision_id,
            retrying.endpoint.clone(),
            retrying.address,
        )),
    );
    let error = executor
        .execute(&request(revision_id, b"body"))
        .await
        .expect_err("429 must be retryable by the owning durable runner");
    assert!(error.is_retryable());
    assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));

    let redirect = HttpFixture::start(AxumStatusCode::FOUND, None, Vec::new()).await;
    let revision_id = ConnectorRevisionId::new();
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            redirect.endpoint.clone(),
            ResolvedConnectorAuthentication::none(),
            1024,
        ),
        Arc::new(ExactEgressAuthorizer::allowing(
            revision_id,
            redirect.endpoint.clone(),
            redirect.address,
        )),
    );
    assert_eq!(
        executor.execute(&request(revision_id, b"body")).await,
        Err(ConnectorExecutionError::Rejected)
    );
    assert_eq!(redirect.requests().await.len(), 1);

    let oversized = HttpFixture::start(AxumStatusCode::OK, None, vec![b'x'; 5]).await;
    let revision_id = ConnectorRevisionId::new();
    let executor = BoundedHttpConnectorExecutor::new(
        resolved_revision(
            revision_id,
            oversized.endpoint.clone(),
            ResolvedConnectorAuthentication::none(),
            4,
        ),
        Arc::new(ExactEgressAuthorizer::allowing(
            revision_id,
            oversized.endpoint.clone(),
            oversized.address,
        )),
    );
    assert_eq!(
        executor.execute(&request(revision_id, b"body")).await,
        Err(ConnectorExecutionError::Rejected)
    );
}

#[test]
fn production_revisions_are_https_only_and_resolved_material_is_bounded() {
    let revision_id = ConnectorRevisionId::new();
    let http = Url::parse("http://example.com/delivery").expect("HTTP URL");
    assert!(ResolvedConnectorHttpRevision::new(
        revision_id,
        http,
        ConnectorHttpMethod::Post,
        "application/json",
        1024,
        1024,
        Duration::from_secs(2),
        ConnectorHttpStatusPolicy::standard_webhook(),
        ResolvedConnectorAuthentication::none(),
    )
    .is_err());
    let user_info =
        Url::parse("https://user:password@example.com/delivery").expect("credential URL");
    assert!(ResolvedConnectorHttpRevision::new(
        revision_id,
        user_info,
        ConnectorHttpMethod::Post,
        "application/json",
        1024,
        1024,
        Duration::from_secs(2),
        ConnectorHttpStatusPolicy::standard_webhook(),
        ResolvedConnectorAuthentication::none(),
    )
    .is_err());
    assert!(ResolvedConnectorAuthentication::hmac_sha256(
        Zeroizing::new(vec![b'x'; MINIMUM_SIGNING_SECRET_BYTES - 1]),
        "x-signature",
        "v1=",
    )
    .is_err());
    assert!(ResolvedConnectorAuthentication::hmac_sha256(
        Zeroizing::new(vec![b'x'; MINIMUM_SIGNING_SECRET_BYTES]),
        "proxy-authorization",
        "v1=",
    )
    .is_err());
}
