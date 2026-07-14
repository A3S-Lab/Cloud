use crate::modules::shared_kernel::application::ApplicationError;
use a3s_boot::{
    BootError, BootResponse, BoxFuture, ExceptionFilter, ExecutionContext, Interceptor, Result,
};
use a3s_cloud_contracts::{ApiErrorResponse, ApiSuccessResponse};
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default)]
pub struct ApiResponseInterceptor;

impl Interceptor for ApiResponseInterceptor {
    fn after(
        &self,
        context: ExecutionContext,
        mut response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(async move {
            if response.is_streaming() || response.is_event_stream() {
                return Ok(response.with_header("x-request-id", request_id(&context).to_string()));
            }
            if response.header("x-a3s-api-envelope") == Some("1") {
                response.headers.remove("x-a3s-api-envelope");
                return Ok(response);
            }
            let request_id = request_id(&context);
            success_response(response, request_id)
        })
    }
}

pub fn application_error_response(
    error: ApplicationError,
    request_id: Uuid,
) -> Result<BootResponse> {
    let (status, status_code, message) = match error {
        ApplicationError::Invalid(message) => (422, "UNPROCESSABLE_ENTITY", message),
        ApplicationError::NotFound(message) => (404, "NOT_FOUND", message),
        ApplicationError::Conflict(message) => (409, "CONFLICT", message),
        ApplicationError::Forbidden(message) => (403, "FORBIDDEN", message),
        ApplicationError::Internal(_) => {
            (500, "INTERNAL_SERVER_ERROR", "Internal server error".into())
        }
    };
    let envelope = ApiErrorResponse {
        code: status,
        status_code: status_code.into(),
        message,
        details: json!({}),
        request_id,
        timestamp: Utc::now(),
    };
    Ok(BootResponse::json_with_status(status, &envelope)?
        .with_header("x-request-id", request_id.to_string())
        .with_header("x-a3s-api-envelope", "1"))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ApiErrorFilter;

impl ExceptionFilter for ApiErrorFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async move { error_response(error, request_id(&context)).map(Some) })
    }
}

fn request_id(context: &ExecutionContext) -> Uuid {
    context
        .request
        .header("x-request-id")
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::new_v4)
}

fn success_response(response: BootResponse, request_id: Uuid) -> Result<BootResponse> {
    let data = if response.body().is_empty() {
        Value::Null
    } else if response
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        serde_json::from_slice(response.body())
            .map_err(|error| BootError::Internal(format!("invalid JSON response: {error}")))?
    } else {
        Value::String(String::from_utf8_lossy(response.body()).into_owned())
    };
    let status = response.status();
    let envelope = ApiSuccessResponse {
        code: status,
        message: "Success".into(),
        data,
        request_id,
        timestamp: Utc::now(),
    };
    copy_headers(
        &response,
        BootResponse::json_with_status(status, &envelope)?,
        request_id,
    )
}

fn error_response(error: BootError, request_id: Uuid) -> Result<BootResponse> {
    let status = error.http_status_code();
    let message = if status >= 500 {
        "Internal server error".to_owned()
    } else {
        error.http_response_message()
    };
    let envelope = ApiErrorResponse {
        code: status,
        status_code: status_code(&error).into(),
        message,
        details: json!({}),
        request_id,
        timestamp: Utc::now(),
    };
    Ok(BootResponse::json_with_status(status, &envelope)?
        .with_header("x-request-id", request_id.to_string()))
}

fn copy_headers(
    source: &BootResponse,
    mut target: BootResponse,
    request_id: Uuid,
) -> Result<BootResponse> {
    for (name, value) in source.header_entries() {
        if name.eq_ignore_ascii_case("content-type")
            || name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("x-request-id")
        {
            continue;
        }
        target = target.append_header(name, value);
    }
    Ok(target.with_header("x-request-id", request_id.to_string()))
}

fn status_code(error: &BootError) -> &'static str {
    match error.http_status_code() {
        400 => "BAD_REQUEST",
        401 => "UNAUTHORIZED",
        403 => "FORBIDDEN",
        404 => "NOT_FOUND",
        405 => "METHOD_NOT_ALLOWED",
        408 => "REQUEST_TIMEOUT",
        409 => "CONFLICT",
        410 => "GONE",
        412 => "PRECONDITION_FAILED",
        413 => "PAYLOAD_TOO_LARGE",
        415 => "UNSUPPORTED_MEDIA_TYPE",
        422 => "UNPROCESSABLE_ENTITY",
        429 => "TOO_MANY_REQUESTS",
        501 => "NOT_IMPLEMENTED",
        502 => "BAD_GATEWAY",
        503 => "SERVICE_UNAVAILABLE",
        504 => "GATEWAY_TIMEOUT",
        _ => "INTERNAL_SERVER_ERROR",
    }
}
