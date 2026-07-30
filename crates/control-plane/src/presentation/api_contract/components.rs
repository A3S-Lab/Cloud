use super::OPENAPI_CONTRACT_VERSION;
use a3s_boot::{BootError, Result};
use serde_json::{json, Map, Value};

pub(super) fn install_components(document: &mut Value) -> Result<()> {
    let document = document
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("generated OpenAPI document is not an object".into()))?;
    let components = document
        .entry("components")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("generated OpenAPI components are invalid".into()))?;
    components.insert(
        "securitySchemes".into(),
        json!({
            "bearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "A3S API token" }
        }),
    );
    components.insert(
        "schemas".into(),
        json!({
            "ApiSuccessResponse": {
                "type": "object",
                "additionalProperties": false,
                "required": ["code", "message", "data", "requestId", "timestamp"],
                "properties": {
                    "code": { "type": "integer", "minimum": 200, "maximum": 399 },
                    "message": { "type": "string" },
                    "data": {},
                    "requestId": { "type": "string", "format": "uuid" },
                    "timestamp": { "type": "string", "format": "date-time" }
                }
            },
            "ApiErrorResponse": {
                "type": "object",
                "additionalProperties": false,
                "required": ["code", "statusCode", "message", "details", "requestId", "timestamp"],
                "properties": {
                    "code": { "type": "integer", "minimum": 400, "maximum": 599 },
                    "statusCode": { "type": "string", "minLength": 1 },
                    "message": { "type": "string" },
                    "details": { "type": "object" },
                    "requestId": { "type": "string", "format": "uuid" },
                    "timestamp": { "type": "string", "format": "date-time" }
                }
            }
        }),
    );

    let mut response_components = Map::new();
    for status in [200, 201, 202, 303] {
        response_components.insert(
            format!("Success{status}"),
            response_component(status, "#/components/schemas/ApiSuccessResponse"),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("SensitiveSuccess{status}"),
            sensitive_response_component(status, "#/components/schemas/ApiSuccessResponse"),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("RawSuccess{status}"),
            response_component(status, ""),
        );
    }
    response_components.insert("SseSuccess200".into(), sse_response_component());
    for status in [400, 401, 403, 404, 409, 422, 429, 500, 503] {
        response_components.insert(
            format!("Error{status}"),
            response_component(status, "#/components/schemas/ApiErrorResponse"),
        );
        response_components.insert(
            format!("SensitiveError{status}"),
            sensitive_response_component(status, "#/components/schemas/ApiErrorResponse"),
        );
    }
    components.insert("responses".into(), Value::Object(response_components));
    Ok(())
}

pub(super) fn response_ref(component: &str) -> Value {
    json!({ "$ref": format!("#/components/responses/{component}") })
}

fn response_component(status: u16, schema_ref: &str) -> Value {
    let schema = if schema_ref.is_empty() {
        json!({ "type": "object", "additionalProperties": true })
    } else {
        json!({ "$ref": schema_ref })
    };
    json!({
        "description": status_description(status),
        "headers": {
            "x-request-id": { "schema": { "type": "string", "format": "uuid" } },
            "x-a3s-api-contract-version": { "schema": { "type": "string", "example": OPENAPI_CONTRACT_VERSION } }
        },
        "content": { "application/json": { "schema": schema } }
    })
}

fn sensitive_response_component(status: u16, schema_ref: &str) -> Value {
    let mut response = response_component(status, schema_ref);
    let headers = response
        .get_mut("headers")
        .and_then(Value::as_object_mut)
        .expect("response component headers are an object");
    headers.insert(
        "cache-control".into(),
        json!({
            "description": "Credential responses must never be stored.",
            "schema": { "type": "string", "enum": ["no-store"] }
        }),
    );
    headers.insert(
        "pragma".into(),
        json!({
            "description": "HTTP/1.0 cache compatibility directive.",
            "schema": { "type": "string", "enum": ["no-cache"] }
        }),
    );
    response
}

fn sse_response_component() -> Value {
    json!({
        "description": "Resumable server-sent event stream",
        "headers": {
            "x-request-id": { "schema": { "type": "string", "format": "uuid" } },
            "x-a3s-api-contract-version": { "schema": { "type": "string", "example": OPENAPI_CONTRACT_VERSION } }
        },
        "content": { "text/event-stream": { "schema": { "type": "string" } } }
    })
}

fn status_description(status: u16) -> &'static str {
    match status {
        200 => "Success or idempotent replay",
        201 => "Created",
        202 => "Accepted",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Response",
    }
}
