use super::*;
use crate::modules::notifications::domain::{
    Notification, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode as AxumStatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

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
}

struct HttpFixture {
    endpoint: Url,
    state: FixtureState,
    server: tokio::task::JoinHandle<()>,
}

impl HttpFixture {
    async fn start(status: AxumStatusCode, retry_after: Option<&str>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let state = FixtureState {
            requests: Arc::new(Mutex::new(Vec::new())),
            response_status: Arc::new(AtomicU16::new(status.as_u16())),
            retry_after: Arc::new(Mutex::new(retry_after.map(str::to_owned))),
        };
        let router = Router::new()
            .route("/delivery/top-secret-endpoint", post(capture_request))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve outbound notification fixture");
        });
        Self {
            endpoint: Url::parse(&format!("http://{address}/delivery/top-secret-endpoint"))
                .expect("fixture endpoint"),
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
    let mut response = status.into_response();
    if let Some(retry_after) = state.retry_after.lock().await.as_deref() {
        response.headers_mut().insert(
            "retry-after",
            retry_after.parse().expect("fixture retry-after header"),
        );
    }
    response
}

fn delivery(
    channel: OutboundNotificationChannel,
    target_revision_id: Uuid,
) -> OutboundNotificationDelivery {
    let now = Utc::now();
    let notification = Notification::project(
        OrganizationId::new(),
        PrincipalId::new(),
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        2,
        Uuid::now_v7(),
        NotificationSeverity::Warning,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        now,
        now,
    )
    .expect("notification");
    OutboundNotificationDelivery::from_notification(&notification, channel, target_revision_id)
        .expect("outbound delivery")
}

#[tokio::test]
async fn signed_webhook_posts_canonical_payload_with_verifiable_redacted_signature() {
    let fixture = HttpFixture::start(AxumStatusCode::NO_CONTENT, None).await;
    let target_revision_id = Uuid::now_v7();
    let secret = b"0123456789abcdef0123456789abcdef";
    let adapter = SignedWebhookNotificationAdapter::for_test(
        target_revision_id,
        fixture.endpoint.clone(),
        Zeroizing::new(secret.to_vec()),
        Duration::from_secs(2),
    )
    .expect("signed webhook adapter");
    let debug = format!("{adapter:?}");
    assert!(!debug.contains("top-secret-endpoint"));
    assert!(!debug.contains("0123456789abcdef"));

    let delivery = delivery(
        OutboundNotificationChannel::SignedWebhook,
        target_revision_id,
    );
    let receipt = adapter.deliver(&delivery).await.expect("webhook delivery");
    assert_eq!(receipt.delivery_id, delivery.id());
    assert_eq!(receipt.target_revision_id, target_revision_id);

    let requests = fixture.requests().await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.body,
        delivery
            .canonical_payload()
            .expect("canonical delivery payload")
    );
    assert_eq!(
        request
            .headers
            .get("x-a3s-notification-delivery-id")
            .and_then(|value| value.to_str().ok()),
        Some(delivery.id().to_string().as_str())
    );
    let timestamp = request
        .headers
        .get("x-a3s-notification-timestamp")
        .and_then(|value| value.to_str().ok())
        .expect("delivery timestamp");
    let signature = request
        .headers
        .get("x-a3s-notification-signature")
        .and_then(|value| value.to_str().ok())
        .expect("delivery signature");
    let expected = webhook_signature(secret, timestamp, delivery.id(), &request.body)
        .expect("expected signature");
    assert_eq!(signature, format!("v1={expected}"));
}

#[tokio::test]
async fn slack_compatible_adapter_reuses_the_bounded_http_transport() {
    let fixture = HttpFixture::start(AxumStatusCode::OK, None).await;
    let target_revision_id = Uuid::now_v7();
    let adapter = SlackCompatibleNotificationAdapter::for_test(
        target_revision_id,
        fixture.endpoint.clone(),
        Duration::from_secs(2),
    )
    .expect("Slack-compatible adapter");
    let delivery = delivery(
        OutboundNotificationChannel::SlackCompatible,
        target_revision_id,
    );
    adapter.deliver(&delivery).await.expect("Slack delivery");

    let requests = fixture.requests().await;
    assert_eq!(requests.len(), 1);
    let payload: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("Slack-compatible payload");
    assert_eq!(
        payload,
        serde_json::json!({
            "text": "[warning] Organization role changed\nYour organization role is now member."
        })
    );
    assert!(requests[0]
        .headers
        .get("x-a3s-notification-signature")
        .is_none());
}

#[tokio::test]
async fn provider_status_is_classified_without_an_adapter_local_retry_loop() {
    let retrying = HttpFixture::start(AxumStatusCode::TOO_MANY_REQUESTS, Some("7")).await;
    let target_revision_id = Uuid::now_v7();
    let adapter = SlackCompatibleNotificationAdapter::for_test(
        target_revision_id,
        retrying.endpoint.clone(),
        Duration::from_secs(2),
    )
    .expect("Slack-compatible adapter");
    let error = adapter
        .deliver(&delivery(
            OutboundNotificationChannel::SlackCompatible,
            target_revision_id,
        ))
        .await
        .expect_err("429 must remain retryable by the shared consumer");
    assert!(error.is_retryable());
    assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));

    let rejected = HttpFixture::start(AxumStatusCode::FOUND, None).await;
    let adapter = SlackCompatibleNotificationAdapter::for_test(
        target_revision_id,
        rejected.endpoint.clone(),
        Duration::from_secs(2),
    )
    .expect("Slack-compatible adapter");
    let error = adapter
        .deliver(&delivery(
            OutboundNotificationChannel::SlackCompatible,
            target_revision_id,
        ))
        .await
        .expect_err("redirects must not be followed");
    assert_eq!(error, OutboundNotificationDeliveryError::Rejected);
}

#[test]
fn production_targets_are_https_only_and_credentials_remain_bounded() {
    let target_revision_id = Uuid::now_v7();
    let http = Url::parse("http://example.com/delivery").expect("HTTP URL");
    assert!(SlackCompatibleNotificationAdapter::new(
        target_revision_id,
        http,
        Duration::from_secs(2)
    )
    .is_err());
    let user_info =
        Url::parse("https://user:password@example.com/delivery").expect("credential-bearing URL");
    assert!(SlackCompatibleNotificationAdapter::new(
        target_revision_id,
        user_info,
        Duration::from_secs(2)
    )
    .is_err());
    let endpoint = Url::parse("https://example.com/delivery").expect("HTTPS URL");
    assert!(SignedWebhookNotificationAdapter::new(
        target_revision_id,
        endpoint,
        Zeroizing::new(vec![b'x'; MINIMUM_SIGNING_SECRET_BYTES - 1]),
        Duration::from_secs(2),
    )
    .is_err());
}
