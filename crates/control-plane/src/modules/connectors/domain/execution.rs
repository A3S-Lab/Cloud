use crate::modules::shared_kernel::domain::ConnectorRevisionId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub(crate) const MAXIMUM_CONNECTOR_BODY_BYTES: usize = 1024 * 1024;
const MAXIMUM_CONNECTOR_HEADER_COUNT: usize = 32;
const MAXIMUM_CONNECTOR_HEADER_NAME_BYTES: usize = 64;
const MAXIMUM_CONNECTOR_HEADER_VALUE_BYTES: usize = 2 * 1024;
const MAXIMUM_CONNECTOR_CONTENT_TYPE_BYTES: usize = 128;
const MAXIMUM_CONNECTOR_SIGNING_INPUT_BYTES: usize = MAXIMUM_CONNECTOR_BODY_BYTES + 4 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionRequest {
    connector_revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    content_type: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    signing_input: Option<Vec<u8>>,
}

impl ConnectorExecutionRequest {
    pub fn new(
        connector_revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
        content_type: impl Into<String>,
        body: Vec<u8>,
    ) -> Result<Self, String> {
        let request = Self {
            connector_revision_id,
            attempt_id,
            content_type: content_type.into(),
            headers: BTreeMap::new(),
            body,
            signing_input: None,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, String> {
        let name = name.into();
        if self.headers.insert(name, value.into()).is_some() {
            return Err("connector request header names must be unique".into());
        }
        self.validate()?;
        Ok(self)
    }

    pub fn with_signing_input(mut self, signing_input: Vec<u8>) -> Result<Self, String> {
        if self.signing_input.is_some() {
            return Err("connector request signing input is already set".into());
        }
        self.signing_input = Some(signing_input);
        self.validate()?;
        Ok(self)
    }

    pub const fn connector_revision_id(&self) -> ConnectorRevisionId {
        self.connector_revision_id
    }

    pub const fn attempt_id(&self) -> Uuid {
        self.attempt_id
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn signing_input(&self) -> Option<&[u8]> {
        self.signing_input.as_deref()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.connector_revision_id.as_uuid().is_nil() || self.attempt_id.is_nil() {
            return Err("connector execution identity is invalid".into());
        }
        if self.body.len() > MAXIMUM_CONNECTOR_BODY_BYTES {
            return Err("connector request body must not exceed 1 MiB".into());
        }
        validate_connector_content_type(&self.content_type)?;
        if self.headers.len() > MAXIMUM_CONNECTOR_HEADER_COUNT {
            return Err("connector request has too many headers".into());
        }
        for (name, value) in &self.headers {
            validate_header(name, value)?;
        }
        if self.signing_input.as_ref().is_some_and(|input| {
            input.is_empty() || input.len() > MAXIMUM_CONNECTOR_SIGNING_INPUT_BYTES
        }) {
            return Err("connector request signing input is invalid".into());
        }
        Ok(())
    }
}

impl fmt::Debug for ConnectorExecutionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionRequest")
            .field("connector_revision_id", &self.connector_revision_id)
            .field("attempt_id", &self.attempt_id)
            .field("content_type", &self.content_type)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("body_bytes", &self.body.len())
            .field(
                "signing_input_bytes",
                &self.signing_input.as_ref().map(Vec::len),
            )
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionReceipt {
    connector_revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    accepted_at: DateTime<Utc>,
    status: u16,
    response_content_type: Option<String>,
    response_body: Vec<u8>,
}

impl ConnectorExecutionReceipt {
    pub fn accepted(
        connector_revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
        accepted_at: DateTime<Utc>,
        status: u16,
        response_content_type: Option<String>,
        response_body: Vec<u8>,
    ) -> Result<Self, ConnectorExecutionError> {
        if connector_revision_id.as_uuid().is_nil()
            || attempt_id.is_nil()
            || !(200..=299).contains(&status)
            || response_body.len() > MAXIMUM_CONNECTOR_BODY_BYTES
        {
            return Err(ConnectorExecutionError::Rejected);
        }
        if let Some(content_type) = &response_content_type {
            validate_connector_content_type(content_type)
                .map_err(|_| ConnectorExecutionError::Rejected)?;
        }
        Ok(Self {
            connector_revision_id,
            attempt_id,
            accepted_at,
            status,
            response_content_type,
            response_body,
        })
    }

    pub const fn connector_revision_id(&self) -> ConnectorRevisionId {
        self.connector_revision_id
    }

    pub const fn attempt_id(&self) -> Uuid {
        self.attempt_id
    }

    pub const fn accepted_at(&self) -> DateTime<Utc> {
        self.accepted_at
    }

    pub const fn status(&self) -> u16 {
        self.status
    }

    pub fn response_content_type(&self) -> Option<&str> {
        self.response_content_type.as_deref()
    }

    pub fn response_body(&self) -> &[u8] {
        &self.response_body
    }
}

impl fmt::Debug for ConnectorExecutionReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionReceipt")
            .field("connector_revision_id", &self.connector_revision_id)
            .field("attempt_id", &self.attempt_id)
            .field("accepted_at", &self.accepted_at)
            .field("status", &self.status)
            .field("response_content_type", &self.response_content_type)
            .field("response_body_bytes", &self.response_body.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorExecutionError {
    #[error("connector target is temporarily unavailable")]
    Retryable { retry_after: Option<Duration> },
    #[error("connector execution was rejected")]
    Rejected,
}

impl ConnectorExecutionError {
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Retryable { retry_after } => *retry_after,
            Self::Rejected => None,
        }
    }
}

#[async_trait]
pub trait IConnectorEgressAuthorizer: Send + Sync {
    /// Re-evaluates the exact materialized destination immediately before each attempt.
    async fn authorize(
        &self,
        connector_revision_id: ConnectorRevisionId,
        endpoint: &Url,
    ) -> Result<(), ConnectorExecutionError>;
}

#[async_trait]
pub trait IConnectorExecutionPort: Send + Sync {
    /// Performs exactly one external attempt. Flow or the owning A3S Event consumer owns
    /// durable attempts, backoff, retry, cancellation, and acknowledgement.
    async fn execute(
        &self,
        request: &ConnectorExecutionRequest,
    ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError>;
}

fn validate_header(name: &str, value: &str) -> Result<(), String> {
    validate_connector_header_name(name)?;
    if forbidden_header(name) {
        return Err("connector request header name is invalid or reserved".into());
    }
    validate_connector_header_value(value)
}

pub(crate) fn validate_connector_header_name(name: &str) -> Result<(), String> {
    let valid_name = !name.is_empty()
        && name.len() <= MAXIMUM_CONNECTOR_HEADER_NAME_BYTES
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (byte == b'-' && index > 0 && index + 1 < name.len())
        });
    if !valid_name {
        return Err("connector header name is invalid".into());
    }
    Ok(())
}

fn validate_connector_header_value(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAXIMUM_CONNECTOR_HEADER_VALUE_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == 0x7f)
    {
        return Err("connector request header value is invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_connector_signature_metadata(
    signature_header: &str,
    value_prefix: &str,
) -> Result<(), String> {
    validate_connector_header_name(signature_header)?;
    if connector_transport_owns_header(signature_header) {
        return Err("connector signature header is reserved by the transport".into());
    }
    if value_prefix.len() > 64
        || value_prefix
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == 0x7f)
    {
        return Err("connector signature value prefix is invalid".into());
    }
    Ok(())
}

fn forbidden_header(name: &str) -> bool {
    connector_transport_owns_header(name) || matches!(name, "authorization" | "cookie")
}

pub(crate) fn connector_transport_owns_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-type"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
    )
}

pub(crate) fn validate_connector_content_type(value: &str) -> Result<(), String> {
    let mut sections = value.split(';');
    let media_type = sections.next().unwrap_or_default();
    let valid_parameters = sections.all(|parameter| {
        !parameter.trim().is_empty()
            && parameter
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
    });
    let valid_media_type = media_type
        .split_once('/')
        .is_some_and(|(kind, subtype)| mime_token(kind) && mime_token(subtype));
    if value.is_empty()
        || value.len() > MAXIMUM_CONNECTOR_CONTENT_TYPE_BYTES
        || value.trim() != value
        || !valid_media_type
        || !valid_parameters
        || value
            .bytes()
            .any(|byte| (!byte.is_ascii_graphic() && byte != b' ') || byte == b',')
    {
        return Err("connector content type is invalid".into());
    }
    Ok(())
}

fn mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rejects_transport_and_credential_authority_from_callers() {
        let revision_id = ConnectorRevisionId::new();
        let request = ConnectorExecutionRequest::new(
            revision_id,
            Uuid::now_v7(),
            "application/json",
            br#"{"ok":true}"#.to_vec(),
        )
        .expect("request");
        assert!(request
            .clone()
            .with_header("authorization", "Bearer copied-secret")
            .is_err());
        assert!(request
            .clone()
            .with_header("Host", "other.example.test")
            .is_err());
        assert!(request
            .with_header("x-a3s-attempt-id", Uuid::now_v7().to_string())
            .is_ok());
    }

    #[test]
    fn request_debug_redacts_body_header_values_and_signing_input() {
        let request = ConnectorExecutionRequest::new(
            ConnectorRevisionId::new(),
            Uuid::now_v7(),
            "application/json",
            b"top-secret-body".to_vec(),
        )
        .expect("request")
        .with_header("x-example", "top-secret-header")
        .expect("header")
        .with_signing_input(b"top-secret-signing-input".to_vec())
        .expect("signing input");
        let debug = format!("{request:?}");
        assert!(!debug.contains("top-secret-body"));
        assert!(!debug.contains("top-secret-header"));
        assert!(!debug.contains("top-secret-signing-input"));
        assert!(debug.contains("x-example"));
    }

    #[test]
    fn request_accepts_empty_bodies_and_content_type_parameters_but_rejects_malformed_values() {
        assert!(ConnectorExecutionRequest::new(
            ConnectorRevisionId::new(),
            Uuid::now_v7(),
            "application/json; charset=utf-8",
            Vec::new(),
        )
        .is_ok());
        assert!(ConnectorExecutionRequest::new(
            ConnectorRevisionId::new(),
            Uuid::now_v7(),
            "application /json",
            Vec::new(),
        )
        .is_err());
        assert!(ConnectorExecutionRequest::new(
            ConnectorRevisionId::new(),
            Uuid::now_v7(),
            "application/json;",
            Vec::new(),
        )
        .is_err());
    }
}
