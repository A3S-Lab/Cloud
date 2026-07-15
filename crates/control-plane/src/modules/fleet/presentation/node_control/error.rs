use crate::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_contracts::{NodeProtocolError, NodeProtocolErrorCode};
use axum::http::{header::HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use uuid::Uuid;

pub(super) struct NodeControlHttpError {
    request_id: Uuid,
    status: StatusCode,
    code: NodeProtocolErrorCode,
    message: String,
    retryable: bool,
}

impl NodeControlHttpError {
    pub(super) fn invalid(request_id: Uuid, message: impl Into<String>) -> Self {
        Self::new(
            request_id,
            StatusCode::BAD_REQUEST,
            NodeProtocolErrorCode::InvalidRequest,
            message,
            false,
        )
    }

    pub(super) fn unauthenticated(request_id: Uuid) -> Self {
        Self::new(
            request_id,
            StatusCode::UNAUTHORIZED,
            NodeProtocolErrorCode::Unauthenticated,
            "node certificate is not active",
            false,
        )
    }

    pub(super) fn payload_too_large(request_id: Uuid, maximum: usize) -> Self {
        Self::new(
            request_id,
            StatusCode::PAYLOAD_TOO_LARGE,
            NodeProtocolErrorCode::PayloadTooLarge,
            format!("request body exceeds the configured {maximum} byte limit"),
            false,
        )
    }

    pub(super) fn request_timeout(request_id: Uuid) -> Self {
        Self::new(
            request_id,
            StatusCode::REQUEST_TIMEOUT,
            NodeProtocolErrorCode::RequestTimeout,
            "request body timed out",
            true,
        )
    }

    pub(super) fn unavailable(request_id: Uuid, message: impl Into<String>) -> Self {
        Self::new(
            request_id,
            StatusCode::SERVICE_UNAVAILABLE,
            NodeProtocolErrorCode::Unavailable,
            message,
            true,
        )
    }

    pub(super) fn internal(request_id: Uuid, message: impl Into<String>) -> Self {
        Self::new(
            request_id,
            StatusCode::INTERNAL_SERVER_ERROR,
            NodeProtocolErrorCode::Internal,
            message,
            true,
        )
    }

    fn new(
        request_id: Uuid,
        status: StatusCode,
        code: NodeProtocolErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            request_id,
            status,
            code,
            message: message.into(),
            retryable,
        }
    }

    pub(super) fn from_application(request_id: Uuid, error: ApplicationError) -> Self {
        match error {
            ApplicationError::Invalid(message) => Self::invalid(request_id, message),
            ApplicationError::NotFound(message) => Self::new(
                request_id,
                StatusCode::NOT_FOUND,
                NodeProtocolErrorCode::NotFound,
                message,
                false,
            ),
            ApplicationError::Conflict(message) => Self::new(
                request_id,
                StatusCode::CONFLICT,
                NodeProtocolErrorCode::Conflict,
                message,
                false,
            ),
            ApplicationError::Forbidden(message) => Self::new(
                request_id,
                StatusCode::FORBIDDEN,
                NodeProtocolErrorCode::Forbidden,
                message,
                false,
            ),
            ApplicationError::Internal(message) => Self::internal(request_id, message),
        }
    }
}

impl IntoResponse for NodeControlHttpError {
    fn into_response(self) -> Response {
        let body = NodeProtocolError::new(
            self.request_id,
            self.code,
            sanitize_message(&self.message),
            self.retryable,
        )
        .unwrap_or_else(|_| NodeProtocolError {
            schema: NodeProtocolError::SCHEMA.into(),
            request_id: self.request_id,
            code: NodeProtocolErrorCode::Internal,
            message: "node-control request failed".into(),
            retryable: true,
        });
        let mut response = (self.status, Json(body)).into_response();
        response.headers_mut().insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(&self.request_id.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("invalid-request-id")),
        );
        response
    }
}

fn sanitize_message(message: &str) -> String {
    let value = message.replace(['\r', '\n', '\0'], " ");
    let value = value.trim();
    if value.is_empty() {
        "node-control request failed".into()
    } else {
        value.chars().take(16 * 1024).collect()
    }
}
