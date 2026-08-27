use crate::modules::shared_kernel::application::ApplicationError;
use crate::presentation::{API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION};
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
            let request_id = request_id(&context);
            response = with_default_private_cache(response);
            if response.is_streaming() || response.is_event_stream() {
                return Ok(response
                    .with_header("x-request-id", request_id.to_string())
                    .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION));
            }
            if response.header("x-a3s-api-envelope") == Some("1") {
                response.headers.remove("x-a3s-api-envelope");
                return Ok(response
                    .with_header("x-request-id", request_id.to_string())
                    .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION));
            }
            success_response(response, request_id)
        })
    }
}

pub fn application_error_response(
    error: ApplicationError,
    request_id: Uuid,
) -> Result<BootResponse> {
    let envelope = application_error_envelope(error, request_id);
    let status = envelope.code;
    Ok(private_no_store(
        BootResponse::json_with_status(status, &envelope)?
            .with_header("x-request-id", request_id.to_string())
            .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION)
            .with_header("x-a3s-api-envelope", "1"),
    ))
}

pub(crate) fn application_error_envelope(
    error: ApplicationError,
    request_id: Uuid,
) -> ApiErrorResponse {
    let (status, status_code, message) = match error {
        ApplicationError::Invalid(message) => (422, "UNPROCESSABLE_ENTITY", message),
        ApplicationError::NotFound(message) => (404, "NOT_FOUND", message),
        ApplicationError::Conflict(message) => (409, "CONFLICT", message),
        ApplicationError::Forbidden(message) => (403, "FORBIDDEN", message),
        ApplicationError::Unavailable(_) => {
            (503, "SERVICE_UNAVAILABLE", "Service unavailable".into())
        }
        ApplicationError::Internal(_) => {
            (500, "INTERNAL_SERVER_ERROR", "Internal server error".into())
        }
    };
    ApiErrorResponse {
        code: status,
        status_code: status_code.into(),
        message,
        details: json!({}),
        request_id,
        timestamp: Utc::now(),
    }
}

pub(crate) fn api_success_envelope<T>(
    status: u16,
    data: T,
    request_id: Uuid,
) -> ApiSuccessResponse<T> {
    ApiSuccessResponse {
        code: status,
        message: "Success".into(),
        data,
        request_id,
        timestamp: Utc::now(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ApiErrorFilter;

impl ExceptionFilter for ApiErrorFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async move { boot_error_response(error, request_id(&context)).map(Some) })
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
    let envelope = api_success_envelope(status, data, request_id);
    copy_headers(
        &response,
        BootResponse::json_with_status(status, &envelope)?,
        request_id,
    )
}

pub(crate) fn boot_error_response(error: BootError, request_id: Uuid) -> Result<BootResponse> {
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
    Ok(private_no_store(
        BootResponse::json_with_status(status, &envelope)?
            .with_header("x-request-id", request_id.to_string())
            .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION),
    ))
}

pub(crate) fn private_no_store(response: BootResponse) -> BootResponse {
    response
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer")
}

fn with_default_private_cache(response: BootResponse) -> BootResponse {
    if response.header("cache-control").is_some() {
        response
    } else {
        private_no_store(response)
    }
}

fn copy_headers(
    source: &BootResponse,
    mut target: BootResponse,
    request_id: Uuid,
) -> Result<BootResponse> {
    for (name, value) in &source.headers {
        if replaced_header(name) {
            continue;
        }
        target = target.with_header(name, value);
    }
    for (name, value) in &source.appended_headers {
        if replaced_header(name) {
            continue;
        }
        target = target.append_header(name, value);
    }
    Ok(target
        .with_header("x-request-id", request_id.to_string())
        .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION))
}

fn replaced_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-type")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("x-request-id")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_responses_are_private_by_default_without_overriding_explicit_cache_policy() {
        let private = with_default_private_cache(
            BootResponse::json(&json!({})).expect("private response fixture"),
        );
        assert_eq!(private.header("cache-control"), Some("no-store"));
        assert_eq!(private.header("pragma"), Some("no-cache"));
        assert_eq!(private.header("referrer-policy"), Some("no-referrer"));

        let public = with_default_private_cache(
            BootResponse::json(&json!({}))
                .expect("public response fixture")
                .with_header("cache-control", "public, max-age=300"),
        );
        assert_eq!(public.header("cache-control"), Some("public, max-age=300"));
        assert_eq!(public.header("pragma"), None);
        assert_eq!(public.header("referrer-policy"), None);
    }
}
