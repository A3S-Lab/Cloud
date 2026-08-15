use crate::modules::connectors::domain::{
    maximum_connector_retry_after, validate_connector_content_type, validate_connector_http_limits,
    validate_connector_signature_metadata, validate_connector_signing_secret_length,
    validate_resolved_connector_endpoint, AuthorizedConnectorDestination, ConnectorExecutionError,
    ConnectorExecutionReceipt, ConnectorExecutionRequest, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, ConnectorStatusDisposition, IConnectorEgressAuthorizer,
    IConnectorExecutionPort, IPreparedConnectorExecution,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorRevisionId, Sha256Digest,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::Utc;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Client, Method};
use sha2::Sha256;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

#[cfg(test)]
use crate::modules::connectors::domain::MINIMUM_SIGNING_SECRET_BYTES;

pub struct ResolvedConnectorAuthentication {
    kind: ResolvedConnectorAuthenticationKind,
}

enum ResolvedConnectorAuthenticationKind {
    None,
    HmacSha256 {
        signing_secret: Zeroizing<Vec<u8>>,
        signature_header: HeaderName,
        value_prefix: String,
    },
}

impl ResolvedConnectorAuthentication {
    pub const fn none() -> Self {
        Self {
            kind: ResolvedConnectorAuthenticationKind::None,
        }
    }

    pub fn hmac_sha256(
        signing_secret: Zeroizing<Vec<u8>>,
        signature_header: impl AsRef<str>,
        value_prefix: impl Into<String>,
    ) -> Result<Self, String> {
        validate_connector_signing_secret_length(signing_secret.len())?;
        let value_prefix = value_prefix.into();
        validate_connector_signature_metadata(signature_header.as_ref(), &value_prefix)?;
        let signature_header = HeaderName::from_bytes(signature_header.as_ref().as_bytes())
            .map_err(|_| "connector signature header is invalid".to_owned())?;
        Ok(Self {
            kind: ResolvedConnectorAuthenticationKind::HmacSha256 {
                signing_secret,
                signature_header,
                value_prefix,
            },
        })
    }
}

impl fmt::Debug for ResolvedConnectorAuthentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ResolvedConnectorAuthenticationKind::None => formatter.write_str("None"),
            ResolvedConnectorAuthenticationKind::HmacSha256 {
                signature_header, ..
            } => formatter
                .debug_struct("HmacSha256")
                .field("signature_header", signature_header)
                .field("material", &"redacted")
                .finish(),
        }
    }
}

pub struct ResolvedConnectorHttpRevision {
    id: ConnectorRevisionId,
    endpoint: Zeroizing<String>,
    method: ConnectorHttpMethod,
    request_content_type: String,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
    timeout: Duration,
    status_policy: ConnectorHttpStatusPolicy,
    authentication: ResolvedConnectorAuthentication,
    allow_http: bool,
}

impl ResolvedConnectorHttpRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ConnectorRevisionId,
        endpoint: Url,
        method: ConnectorHttpMethod,
        request_content_type: impl Into<String>,
        maximum_request_bytes: usize,
        maximum_response_bytes: usize,
        timeout: Duration,
        status_policy: ConnectorHttpStatusPolicy,
        authentication: ResolvedConnectorAuthentication,
    ) -> Result<Self, String> {
        Self::with_transport(
            id,
            endpoint,
            method,
            request_content_type.into(),
            maximum_request_bytes,
            maximum_response_bytes,
            timeout,
            status_policy,
            authentication,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_transport(
        id: ConnectorRevisionId,
        endpoint: Url,
        method: ConnectorHttpMethod,
        request_content_type: String,
        maximum_request_bytes: usize,
        maximum_response_bytes: usize,
        timeout: Duration,
        status_policy: ConnectorHttpStatusPolicy,
        authentication: ResolvedConnectorAuthentication,
        allow_http: bool,
    ) -> Result<Self, String> {
        if id.as_uuid().is_nil() {
            return Err("resolved connector HTTP destination is invalid".into());
        }
        validate_resolved_connector_endpoint(&endpoint, allow_http, true)?;
        validate_connector_content_type(&request_content_type)?;
        validate_connector_http_limits(maximum_request_bytes, maximum_response_bytes, timeout)?;
        status_policy.validate()?;
        Ok(Self {
            id,
            endpoint: Zeroizing::new(endpoint.to_string()),
            method,
            request_content_type,
            maximum_request_bytes,
            maximum_response_bytes,
            timeout,
            status_policy,
            authentication,
            allow_http,
        })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn for_test(
        id: ConnectorRevisionId,
        endpoint: Url,
        method: ConnectorHttpMethod,
        request_content_type: impl Into<String>,
        maximum_request_bytes: usize,
        maximum_response_bytes: usize,
        timeout: Duration,
        status_policy: ConnectorHttpStatusPolicy,
        authentication: ResolvedConnectorAuthentication,
    ) -> Result<Self, String> {
        Self::with_transport(
            id,
            endpoint,
            method,
            request_content_type.into(),
            maximum_request_bytes,
            maximum_response_bytes,
            timeout,
            status_policy,
            authentication,
            true,
        )
    }
}

impl fmt::Debug for ResolvedConnectorHttpRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedConnectorHttpRevision")
            .field("id", &self.id)
            .field("method", &self.method)
            .field("request_content_type", &self.request_content_type)
            .field("maximum_request_bytes", &self.maximum_request_bytes)
            .field("maximum_response_bytes", &self.maximum_response_bytes)
            .field("timeout", &self.timeout)
            .field("authentication", &self.authentication)
            .field("destination", &"redacted")
            .finish()
    }
}

pub struct BoundedHttpConnectorExecutor {
    revision: ResolvedConnectorHttpRevision,
    egress: Arc<dyn IConnectorEgressAuthorizer>,
}

impl BoundedHttpConnectorExecutor {
    pub fn new(
        revision: ResolvedConnectorHttpRevision,
        egress: Arc<dyn IConnectorEgressAuthorizer>,
    ) -> Self {
        Self { revision, egress }
    }

    pub(super) async fn prepare(
        self,
        request: &ConnectorExecutionRequest,
    ) -> Result<PreparedBoundedHttpConnectorExecution, ConnectorExecutionError> {
        let endpoint = validate_request(&self.revision, request)?;
        let authorized = self.egress.authorize(self.revision.id, &endpoint).await?;
        Ok(PreparedBoundedHttpConnectorExecution {
            revision: self.revision,
            authorized,
            attempt_id: request.attempt_id(),
            request_digest: request.evidence_digest(),
        })
    }
}

impl fmt::Debug for BoundedHttpConnectorExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedHttpConnectorExecutor")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IConnectorExecutionPort for BoundedHttpConnectorExecutor {
    async fn execute(
        &self,
        request: &ConnectorExecutionRequest,
    ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
        let endpoint = validate_request(&self.revision, request)?;
        let authorized = self.egress.authorize(self.revision.id, &endpoint).await?;
        execute_authorized(&self.revision, &authorized, request).await
    }
}

pub(super) struct PreparedBoundedHttpConnectorExecution {
    revision: ResolvedConnectorHttpRevision,
    authorized: AuthorizedConnectorDestination,
    attempt_id: Uuid,
    request_digest: Sha256Digest,
}

impl fmt::Debug for PreparedBoundedHttpConnectorExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedBoundedHttpConnectorExecution")
            .field("revision_id", &self.revision.id)
            .field("attempt_id", &self.attempt_id)
            .field("outcome_timeout", &self.revision.timeout)
            .field("request", &"redacted")
            .field("destination", &"redacted")
            .finish()
    }
}

#[async_trait]
impl IPreparedConnectorExecution for PreparedBoundedHttpConnectorExecution {
    fn outcome_timeout(&self) -> Duration {
        self.revision.timeout
    }

    async fn dispatch(
        self: Box<Self>,
        request: &ConnectorExecutionRequest,
    ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
        if request.attempt_id() != self.attempt_id
            || request.evidence_digest() != self.request_digest
        {
            return Err(ConnectorExecutionError::Rejected);
        }
        execute_authorized(&self.revision, &self.authorized, request).await
    }
}

fn validate_request(
    revision: &ResolvedConnectorHttpRevision,
    request: &ConnectorExecutionRequest,
) -> Result<Url, ConnectorExecutionError> {
    request
        .validate()
        .map_err(|_| ConnectorExecutionError::Rejected)?;
    if request.connector_revision_id() != revision.id
        || request.content_type() != revision.request_content_type
        || request.body().len() > revision.maximum_request_bytes
    {
        return Err(ConnectorExecutionError::Rejected);
    }
    Url::parse(revision.endpoint.as_str()).map_err(|_| ConnectorExecutionError::Rejected)
}

async fn execute_authorized(
    revision: &ResolvedConnectorHttpRevision,
    authorized: &AuthorizedConnectorDestination,
    request: &ConnectorExecutionRequest,
) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
    let endpoint = validate_request(revision, request)?;
    let client = attempt_http_client(revision, &endpoint, authorized)?;

    let mut headers = request_headers(request)?;
    apply_authentication(&revision.authentication, request, &mut headers)?;
    let response = client
        .request(http_method(revision.method), endpoint)
        .headers(headers)
        .body(request.body().to_vec())
        .send()
        .await
        .map_err(|_| ConnectorExecutionError::Retryable { retry_after: None })?;
    let status = response.status().as_u16();
    match revision.status_policy.classify(status) {
        ConnectorStatusDisposition::Retryable => {
            return Err(ConnectorExecutionError::Retryable {
                retry_after: retry_after(response.headers()),
            });
        }
        ConnectorStatusDisposition::Rejected => {
            return Err(ConnectorExecutionError::Rejected);
        }
        ConnectorStatusDisposition::Accepted => {}
    }

    let response_content_type = response_content_type(response.headers())?;
    let response_body = bounded_response_body(response, revision.maximum_response_bytes).await?;
    ConnectorExecutionReceipt::accepted(
        revision.id,
        request.attempt_id(),
        canonical_timestamp(Utc::now()),
        status,
        response_content_type,
        response_body,
    )
}

fn attempt_http_client(
    revision: &ResolvedConnectorHttpRevision,
    endpoint: &Url,
    authorized: &AuthorizedConnectorDestination,
) -> Result<Client, ConnectorExecutionError> {
    if !authorized.matches_endpoint(endpoint) {
        return Err(ConnectorExecutionError::Rejected);
    }
    let host = endpoint
        .host_str()
        .ok_or(ConnectorExecutionError::Rejected)?;
    Client::builder()
        .use_rustls_tls()
        .timeout(revision.timeout)
        .connect_timeout(revision.timeout)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(!revision.allow_http)
        .no_proxy()
        .resolve_to_addrs(host, authorized.socket_addresses())
        .user_agent("a3s-cloud-connectors")
        .build()
        .map_err(|_| ConnectorExecutionError::Retryable { retry_after: None })
}

fn request_headers(
    request: &ConnectorExecutionRequest,
) -> Result<HeaderMap, ConnectorExecutionError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value(request.content_type())?);
    for (name, value) in request.headers() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ConnectorExecutionError::Rejected)?;
        if headers.insert(name, header_value(value)?).is_some() {
            return Err(ConnectorExecutionError::Rejected);
        }
    }
    Ok(headers)
}

fn apply_authentication(
    authentication: &ResolvedConnectorAuthentication,
    request: &ConnectorExecutionRequest,
    headers: &mut HeaderMap,
) -> Result<(), ConnectorExecutionError> {
    match (&authentication.kind, request.signing_input()) {
        (ResolvedConnectorAuthenticationKind::None, None) => Ok(()),
        (
            ResolvedConnectorAuthenticationKind::HmacSha256 {
                signing_secret,
                signature_header,
                value_prefix,
            },
            Some(signing_input),
        ) => {
            if headers.contains_key(signature_header) {
                return Err(ConnectorExecutionError::Rejected);
            }
            let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_slice())
                .map_err(|_| ConnectorExecutionError::Rejected)?;
            mac.update(signing_input);
            let signature = BASE64_STANDARD.encode(mac.finalize().into_bytes());
            headers.insert(
                signature_header.clone(),
                header_value(&format!("{value_prefix}{signature}"))?,
            );
            Ok(())
        }
        _ => Err(ConnectorExecutionError::Rejected),
    }
}

async fn bounded_response_body(
    response: reqwest::Response,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ConnectorExecutionError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ConnectorExecutionError::Retryable { retry_after: None })?;
        if body.len().saturating_add(chunk.len()) > maximum_bytes {
            return Err(ConnectorExecutionError::Rejected);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn response_content_type(headers: &HeaderMap) -> Result<Option<String>, ConnectorExecutionError> {
    headers
        .get(CONTENT_TYPE)
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| ConnectorExecutionError::Rejected)?;
            validate_connector_content_type(value)
                .map_err(|_| ConnectorExecutionError::Rejected)?;
            Ok(value.to_owned())
        })
        .transpose()
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|duration| *duration <= maximum_connector_retry_after())
}

fn header_value(value: &str) -> Result<HeaderValue, ConnectorExecutionError> {
    HeaderValue::from_str(value).map_err(|_| ConnectorExecutionError::Rejected)
}

fn http_method(method: ConnectorHttpMethod) -> Method {
    match method {
        ConnectorHttpMethod::Get => Method::GET,
        ConnectorHttpMethod::Post => Method::POST,
        ConnectorHttpMethod::Put => Method::PUT,
        ConnectorHttpMethod::Patch => Method::PATCH,
        ConnectorHttpMethod::Delete => Method::DELETE,
    }
}

#[cfg(test)]
#[path = "http_executor_tests.rs"]
mod tests;
