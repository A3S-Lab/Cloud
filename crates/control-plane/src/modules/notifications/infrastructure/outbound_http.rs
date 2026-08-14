use crate::modules::notifications::domain::{
    IOutboundNotificationAdapter, OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationDeliveryError, OutboundNotificationDeliveryReceipt,
};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use sha2::Sha256;
use std::fmt;
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const MINIMUM_SIGNING_SECRET_BYTES: usize = 32;
const MAXIMUM_SIGNING_SECRET_BYTES: usize = 4 * 1024;
const MAXIMUM_ENDPOINT_CHARACTERS: usize = 2_048;
const MAXIMUM_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAXIMUM_RETRY_AFTER: Duration = Duration::from_secs(86_400);

#[derive(Clone)]
struct BoundedNotificationHttpClient {
    client: Client,
}

impl BoundedNotificationHttpClient {
    fn new(timeout: Duration, allow_http: bool) -> Result<Self, String> {
        if timeout.is_zero() || timeout > MAXIMUM_REQUEST_TIMEOUT {
            return Err("outbound notification timeout must be between 1 ms and 60 seconds".into());
        }
        let client = Client::builder()
            .use_rustls_tls()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!allow_http)
            .user_agent("a3s-cloud-notifications")
            .build()
            .map_err(|_| "could not build outbound notification HTTP client".to_owned())?;
        Ok(Self { client })
    }

    async fn post(
        &self,
        endpoint: &Url,
        body: Vec<u8>,
        headers: HeaderMap,
    ) -> Result<(), OutboundNotificationDeliveryError> {
        let response = self
            .client
            .post(endpoint.clone())
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|_| OutboundNotificationDeliveryError::Retryable { retry_after: None })?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        if retryable_status(status) {
            return Err(OutboundNotificationDeliveryError::Retryable {
                retry_after: retry_after(response.headers()),
            });
        }
        Err(OutboundNotificationDeliveryError::Rejected)
    }
}

pub struct SignedWebhookNotificationAdapter {
    client: BoundedNotificationHttpClient,
    target_revision_id: Uuid,
    endpoint: Url,
    signing_secret: Zeroizing<Vec<u8>>,
}

impl SignedWebhookNotificationAdapter {
    pub fn new(
        target_revision_id: Uuid,
        endpoint: Url,
        signing_secret: Zeroizing<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Self, String> {
        Self::with_transport(target_revision_id, endpoint, signing_secret, timeout, false)
    }

    fn with_transport(
        target_revision_id: Uuid,
        endpoint: Url,
        signing_secret: Zeroizing<Vec<u8>>,
        timeout: Duration,
        allow_http: bool,
    ) -> Result<Self, String> {
        validate_target(target_revision_id, &endpoint, allow_http)?;
        if !(MINIMUM_SIGNING_SECRET_BYTES..=MAXIMUM_SIGNING_SECRET_BYTES)
            .contains(&signing_secret.len())
        {
            return Err("signed webhook secret must contain between 32 and 4096 bytes".into());
        }
        Ok(Self {
            client: BoundedNotificationHttpClient::new(timeout, allow_http)?,
            target_revision_id,
            endpoint,
            signing_secret,
        })
    }

    #[cfg(test)]
    fn for_test(
        target_revision_id: Uuid,
        endpoint: Url,
        signing_secret: Zeroizing<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Self, String> {
        Self::with_transport(target_revision_id, endpoint, signing_secret, timeout, true)
    }
}

impl fmt::Debug for SignedWebhookNotificationAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignedWebhookNotificationAdapter")
            .field("target_revision_id", &self.target_revision_id)
            .field("transport", &"https")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IOutboundNotificationAdapter for SignedWebhookNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SignedWebhook
    }

    fn target_revision_id(&self) -> Uuid {
        self.target_revision_id
    }

    async fn deliver(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError> {
        validate_delivery(delivery, self.channel(), self.target_revision_id)?;
        let body = delivery
            .canonical_payload()
            .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
        let attempted_at = canonical_timestamp(Utc::now());
        let timestamp = attempted_at.to_rfc3339_opts(SecondsFormat::Micros, true);
        let signature = webhook_signature(
            self.signing_secret.as_slice(),
            &timestamp,
            delivery.id(),
            &body,
        )?;
        let mut headers = delivery_headers(delivery.id())?;
        headers.insert("x-a3s-notification-timestamp", header_value(&timestamp)?);
        headers.insert(
            "x-a3s-notification-signature",
            header_value(&format!("v1={signature}"))?,
        );
        self.client.post(&self.endpoint, body, headers).await?;
        Ok(OutboundNotificationDeliveryReceipt {
            delivery_id: delivery.id(),
            target_revision_id: self.target_revision_id,
            accepted_at: canonical_timestamp(Utc::now()),
        })
    }
}

pub struct SlackCompatibleNotificationAdapter {
    client: BoundedNotificationHttpClient,
    target_revision_id: Uuid,
    endpoint: Url,
}

impl SlackCompatibleNotificationAdapter {
    pub fn new(target_revision_id: Uuid, endpoint: Url, timeout: Duration) -> Result<Self, String> {
        Self::with_transport(target_revision_id, endpoint, timeout, false)
    }

    fn with_transport(
        target_revision_id: Uuid,
        endpoint: Url,
        timeout: Duration,
        allow_http: bool,
    ) -> Result<Self, String> {
        validate_target(target_revision_id, &endpoint, allow_http)?;
        Ok(Self {
            client: BoundedNotificationHttpClient::new(timeout, allow_http)?,
            target_revision_id,
            endpoint,
        })
    }

    #[cfg(test)]
    fn for_test(
        target_revision_id: Uuid,
        endpoint: Url,
        timeout: Duration,
    ) -> Result<Self, String> {
        Self::with_transport(target_revision_id, endpoint, timeout, true)
    }
}

impl fmt::Debug for SlackCompatibleNotificationAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SlackCompatibleNotificationAdapter")
            .field("target_revision_id", &self.target_revision_id)
            .field("transport", &"https")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IOutboundNotificationAdapter for SlackCompatibleNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SlackCompatible
    }

    fn target_revision_id(&self) -> Uuid {
        self.target_revision_id
    }

    async fn deliver(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError> {
        validate_delivery(delivery, self.channel(), self.target_revision_id)?;
        let body = crate::modules::shared_kernel::domain::canonical_json_bounded(
            &SlackCompatiblePayload {
                text: format!(
                    "[{}] {}\n{}",
                    delivery.severity().as_str(),
                    delivery.title(),
                    delivery.body()
                ),
            },
            16 * 1024,
            "Slack-compatible notification payload",
        )
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
        self.client
            .post(&self.endpoint, body, delivery_headers(delivery.id())?)
            .await?;
        Ok(OutboundNotificationDeliveryReceipt {
            delivery_id: delivery.id(),
            target_revision_id: self.target_revision_id,
            accepted_at: canonical_timestamp(Utc::now()),
        })
    }
}

#[derive(Serialize)]
struct SlackCompatiblePayload {
    text: String,
}

fn validate_delivery(
    delivery: &OutboundNotificationDelivery,
    channel: OutboundNotificationChannel,
    target_revision_id: Uuid,
) -> Result<(), OutboundNotificationDeliveryError> {
    delivery
        .validate()
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
    if delivery.channel() != channel || delivery.target_revision_id() != target_revision_id {
        return Err(OutboundNotificationDeliveryError::Rejected);
    }
    Ok(())
}

fn validate_target(
    target_revision_id: Uuid,
    endpoint: &Url,
    allow_http: bool,
) -> Result<(), String> {
    let accepted_scheme =
        endpoint.scheme() == "https" || (allow_http && endpoint.scheme() == "http");
    if target_revision_id.is_nil()
        || !accepted_scheme
        || endpoint.as_str().chars().count() > MAXIMUM_ENDPOINT_CHARACTERS
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err("outbound notification target is invalid".into());
    }
    Ok(())
}

fn delivery_headers(delivery_id: Uuid) -> Result<HeaderMap, OutboundNotificationDeliveryError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "x-a3s-notification-delivery-id",
        header_value(&delivery_id.to_string())?,
    );
    Ok(headers)
}

fn header_value(value: &str) -> Result<HeaderValue, OutboundNotificationDeliveryError> {
    HeaderValue::from_str(value).map_err(|_| OutboundNotificationDeliveryError::Rejected)
}

fn webhook_signature(
    secret: &[u8],
    timestamp: &str,
    delivery_id: Uuid,
    body: &[u8],
) -> Result<String, OutboundNotificationDeliveryError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
    mac.update(b"v1\n");
    mac.update(timestamp.as_bytes());
    mac.update(b"\n");
    mac.update(delivery_id.to_string().as_bytes());
    mac.update(b"\n");
    mac.update(body);
    Ok(BASE64_STANDARD.encode(mac.finalize().into_bytes()))
}

fn retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_EARLY | StatusCode::TOO_MANY_REQUESTS
    ) || status.is_server_error()
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|duration| *duration <= MAXIMUM_RETRY_AFTER)
}

#[cfg(test)]
#[path = "outbound_http_tests.rs"]
mod tests;
