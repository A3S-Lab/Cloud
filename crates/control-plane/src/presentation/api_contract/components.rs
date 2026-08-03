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
            format!("RawSuccess{status}"),
            response_component(status, ""),
        );
    }
    response_components.insert(
        "AssetGitAdvertisementSuccess200".into(),
        asset_git_response_component(
            "Git Smart HTTP reference advertisement",
            &[
                "application/x-git-upload-pack-advertisement",
                "application/x-git-receive-pack-advertisement",
            ],
        ),
    );
    response_components.insert(
        "AssetGitUploadPackSuccess200".into(),
        asset_git_response_component(
            "Git Smart HTTP upload-pack result",
            &["application/x-git-upload-pack-result"],
        ),
    );
    response_components.insert(
        "AssetGitReceivePackSuccess200".into(),
        asset_git_response_component(
            "Git Smart HTTP receive-pack result",
            &["application/x-git-receive-pack-result"],
        ),
    );
    response_components.insert("SseSuccess200".into(), sse_response_component());
    for status in [400, 401, 403, 404, 409, 413, 415, 422, 429, 500, 503] {
        response_components.insert(
            format!("Error{status}"),
            response_component(status, "#/components/schemas/ApiErrorResponse"),
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

fn asset_git_response_component(description: &str, media_types: &[&str]) -> Value {
    let content = media_types
        .iter()
        .map(|media_type| {
            (
                (*media_type).to_owned(),
                json!({ "schema": { "type": "string", "format": "binary" } }),
            )
        })
        .collect::<Map<String, Value>>();
    json!({
        "description": description,
        "headers": {
            "x-request-id": { "schema": { "type": "string", "format": "uuid" } },
            "x-a3s-api-contract-version": { "schema": { "type": "string", "example": OPENAPI_CONTRACT_VERSION } }
        },
        "content": content
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
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Response",
    }
}
