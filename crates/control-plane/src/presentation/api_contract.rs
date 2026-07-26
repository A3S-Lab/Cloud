use a3s_boot::{
    BootApplication, BootError, BootResponse, Module, OpenApiInfo, Result, RouteDefinition,
    AUTH_PUBLIC_METADATA,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const API_PREFIX: &str = "/api/v1";
pub const API_MAJOR_VERSION: u16 = 1;
pub const OPENAPI_CONTRACT_VERSION: &str = "1.0.0";
pub const OPENAPI_DOCUMENT_PATH: &str = "/openapi.json";
pub const OPENAPI_PUBLIC_PATH: &str = "/api/v1/openapi.json";
pub const API_CONTRACT_VERSION_HEADER: &str = "x-a3s-api-contract-version";
pub const MINIMUM_DEPRECATION_DAYS: u16 = 180;

const OPENAPI_DOCUMENT: &str = include_str!("../../../../openapi/v1.json");
const HTTP_METHODS: [&str; 7] = ["delete", "get", "head", "options", "patch", "post", "put"];

#[derive(Debug, Clone, Copy, Default)]
pub struct ApiContractModule;

impl Module for ApiContractModule {
    fn name(&self) -> &'static str {
        "api-contract"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            OPENAPI_DOCUMENT_PATH,
            |_| async {
                Ok(BootResponse::new(200, OPENAPI_DOCUMENT.as_bytes())
                    .with_header("content-type", "application/json")
                    .with_header("cache-control", "public, max-age=300")
                    .with_header(API_CONTRACT_VERSION_HEADER, OPENAPI_CONTRACT_VERSION)
                    .with_header("x-a3s-api-envelope", "1"))
            },
        )?
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .hide_from_openapi()])
    }
}

pub fn openapi_info() -> OpenApiInfo {
    OpenApiInfo::new("A3S Cloud REST API", OPENAPI_CONTRACT_VERSION)
        .with_description(
            "Stable version 1 REST contract shared by the A3S Cloud web console and CLI.",
        )
        .with_server_description(API_PREFIX, "A3S Cloud REST API v1")
}

pub fn generate_openapi_contract(application: &BootApplication) -> Result<Value> {
    let mut document =
        serde_json::to_value(application.openapi(openapi_info())).map_err(|error| {
            BootError::Internal(format!("failed to serialize the OpenAPI contract: {error}"))
        })?;
    document["x-a3s-api-major-version"] = json!(API_MAJOR_VERSION);
    document["x-a3s-api-contract-version"] = json!(OPENAPI_CONTRACT_VERSION);
    document["x-a3s-minimum-deprecation-days"] = json!(MINIMUM_DEPRECATION_DAYS);

    let public_operations = public_operations(application);
    normalize_and_describe_paths(&mut document, &public_operations)?;
    install_components(&mut document)?;
    Ok(document)
}

fn public_operations(application: &BootApplication) -> BTreeSet<(String, String)> {
    application
        .routes()
        .iter()
        .filter(|route| !route.openapi().hidden)
        .filter(|route| route.metadata_value(AUTH_PUBLIC_METADATA) == Some(&Value::Bool(true)))
        .map(|route| {
            (
                normalize_route_path(route.path()),
                route.method().as_str().to_ascii_lowercase(),
            )
        })
        .collect()
}

fn normalize_and_describe_paths(
    document: &mut Value,
    public_operations: &BTreeSet<(String, String)>,
) -> Result<()> {
    let paths = document
        .get_mut("paths")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("generated OpenAPI document has no paths".into()))?;
    let generated = std::mem::take(paths);
    let mut normalized = Map::new();

    for (full_path, mut path_item) in generated {
        let path = strip_api_prefix(&full_path)?;
        let operations = path_item.as_object_mut().ok_or_else(|| {
            BootError::Internal(format!("OpenAPI path `{full_path}` is not an object"))
        })?;
        for method in HTTP_METHODS {
            let Some(operation) = operations.get_mut(method) else {
                continue;
            };
            let is_public = public_operations.contains(&(full_path.clone(), method.to_owned()));
            describe_operation(operation, method, &path, is_public)?;
        }
        normalized.insert(path, path_item);
    }

    document["paths"] = Value::Object(normalized);
    Ok(())
}

fn describe_operation(
    operation: &mut Value,
    method: &str,
    path: &str,
    is_public: bool,
) -> Result<()> {
    let operation = operation.as_object_mut().ok_or_else(|| {
        BootError::Internal(format!(
            "OpenAPI operation `{method} {path}` is not an object"
        ))
    })?;
    let operation_id = operation_id(method, path);
    operation.insert("operationId".into(), json!(operation_id));
    operation.insert(
        "summary".into(),
        json!(format!("{} {path}", method.to_ascii_uppercase())),
    );
    operation.insert("tags".into(), json!([operation_tag(path)]));
    operation.insert("x-a3s-stability".into(), json!("stable"));
    operation.insert(
        "x-a3s-api-contract-version".into(),
        json!(OPENAPI_CONTRACT_VERSION),
    );
    operation.insert(
        "security".into(),
        if is_public {
            json!([])
        } else {
            json!([{ "bearerAuth": [] }])
        },
    );

    describe_parameters(operation, method, path)?;
    describe_request_body(operation, method, path);
    operation.insert("responses".into(), responses(method, path, is_public));
    Ok(())
}

fn describe_parameters(operation: &mut Map<String, Value>, method: &str, path: &str) -> Result<()> {
    let parameters = operation
        .entry("parameters")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            BootError::Internal(format!(
                "OpenAPI parameters for `{method} {path}` are not an array"
            ))
        })?;

    for parameter in parameters.iter_mut() {
        let Some(parameter) = parameter.as_object_mut() else {
            continue;
        };
        let is_identifier = parameter
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.ends_with("_id") && name != "unit_id");
        if is_identifier {
            parameter.insert(
                "schema".into(),
                json!({ "type": "string", "format": "uuid" }),
            );
        } else if parameter.get("name").and_then(Value::as_str) == Some("version") {
            parameter.insert("schema".into(), json!({ "type": "integer", "minimum": 1 }));
        }
    }

    if requires_idempotency_key(method, path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "idempotency-key",
                "in": "header",
                "required": true,
                "description": "Caller-owned replay key for this mutation.",
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 255,
                    "pattern": "^[A-Za-z0-9._~:/-]+$"
                }
            }),
        );
    }
    if path == "/bootstrap" {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-bootstrap-token",
                "in": "header",
                "required": true,
                "schema": { "type": "string", "minLength": 32 }
            }),
        );
    }
    if path == "/webhooks/github" {
        for name in ["x-github-event", "x-github-delivery", "x-hub-signature-256"] {
            upsert_parameter(
                parameters,
                json!({
                    "name": name,
                    "in": "header",
                    "required": true,
                    "schema": { "type": "string", "minLength": 1 }
                }),
            );
        }
    }
    describe_query_parameters(parameters, path);
    Ok(())
}

fn describe_query_parameters(parameters: &mut Vec<Value>, path: &str) {
    if path.ends_with("/search") {
        upsert_parameter(
            parameters,
            json!({
                "name": "q", "in": "query", "required": true,
                "schema": { "type": "string", "minLength": 1, "maxLength": 128 }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
            }),
        );
    }
    if path.ends_with("/logs") || path.ends_with("/logs/stream") {
        let maximum = if path.ends_with("/stream") { 16 } else { 256 };
        upsert_parameter(
            parameters,
            json!({
                "name": "cursor", "in": "query", "required": false,
                "schema": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": maximum }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "stream", "in": "query", "required": false,
                "schema": { "type": "string", "enum": ["stdout", "stderr"] }
            }),
        );
    }
    if path.ends_with("/operations") || path.ends_with("/build-runs") {
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 200 }
            }),
        );
    }
    if path.ends_with("/stream") {
        upsert_parameter(
            parameters,
            json!({
                "name": "last-event-id", "in": "header", "required": false,
                "description": "Resume after the last delivered event identifier.",
                "schema": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
        );
    }
    if path == "/source-connections/github/setup" {
        upsert_parameter(
            parameters,
            json!({
                "name": "installation_id", "in": "query", "required": true,
                "schema": { "type": "integer", "minimum": 1 }
            }),
        );
        for (name, required) in [("state", true), ("setup_action", false)] {
            upsert_parameter(
                parameters,
                json!({
                    "name": name, "in": "query", "required": required,
                    "schema": { "type": "string", "minLength": 1 }
                }),
            );
        }
    }
    if path == "/source-connections/github/callback" {
        for name in ["code", "state", "error"] {
            upsert_parameter(
                parameters,
                json!({
                    "name": name, "in": "query", "required": false,
                    "schema": { "type": "string" }
                }),
            );
        }
    }
}

fn describe_request_body(operation: &mut Map<String, Value>, method: &str, path: &str) {
    if method != "post" || request_has_no_body(path) {
        return;
    }
    let mut content = Map::new();
    content.insert(
        "application/json".into(),
        json!({ "schema": { "type": "object", "additionalProperties": true } }),
    );
    if accepts_acl(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({ "schema": { "type": "string", "minLength": 1 } }),
        );
    }
    operation.insert(
        "requestBody".into(),
        json!({
            "required": true,
            "content": content
        }),
    );
}

fn responses(method: &str, path: &str, is_public: bool) -> Value {
    let mut responses = Map::new();
    for status in success_statuses(method, path) {
        let component = if path.ends_with("/stream") {
            "SseSuccess200".to_owned()
        } else if path == "/node-control/enroll" {
            format!("RawSuccess{status}")
        } else {
            format!("Success{status}")
        };
        responses.insert(status.to_string(), response_ref(&component));
    }
    for status in [400, 404, 409, 422, 429, 500, 503] {
        responses.insert(status.to_string(), response_ref(&format!("Error{status}")));
    }
    if !is_public || path == "/webhooks/github" {
        responses.insert("401".into(), response_ref("Error401"));
    }
    if !is_public {
        responses.insert("403".into(), response_ref("Error403"));
    }
    Value::Object(responses)
}

fn success_statuses(method: &str, path: &str) -> Vec<u16> {
    if path == "/source-connections/github/setup" {
        return vec![303];
    }
    if path == "/source-connections/github/callback" {
        return vec![201];
    }
    if path == "/webhooks/github" {
        return vec![202];
    }
    if method == "get" {
        return vec![200];
    }
    if method == "delete" && (path.contains("/deployments/") || path.contains("/build-runs/")) {
        return vec![200, 202];
    }
    if method == "post" && asynchronous_mutation(path) {
        return vec![200, 202];
    }
    if method == "post" && creates_resource(path) {
        return vec![200, 201];
    }
    vec![200]
}

fn install_components(document: &mut Value) -> Result<()> {
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
    response_components.insert("SseSuccess200".into(), sse_response_component());
    for status in [400, 401, 403, 404, 409, 422, 429, 500, 503] {
        response_components.insert(
            format!("Error{status}"),
            response_component(status, "#/components/schemas/ApiErrorResponse"),
        );
    }
    components.insert("responses".into(), Value::Object(response_components));
    Ok(())
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

fn response_ref(component: &str) -> Value {
    json!({ "$ref": format!("#/components/responses/{component}") })
}

fn upsert_parameter(parameters: &mut Vec<Value>, candidate: Value) {
    let identity = parameter_identity(&candidate);
    if parameters
        .iter()
        .any(|parameter| parameter_identity(parameter) == identity)
    {
        return;
    }
    parameters.push(candidate);
}

fn parameter_identity(parameter: &Value) -> Option<(&str, &str)> {
    Some((
        parameter.get("in")?.as_str()?,
        parameter.get("name")?.as_str()?,
    ))
}

fn strip_api_prefix(path: &str) -> Result<String> {
    let stripped = path.strip_prefix(API_PREFIX).ok_or_else(|| {
        BootError::Internal(format!("public route `{path}` is outside `{API_PREFIX}`"))
    })?;
    Ok(if stripped.is_empty() {
        "/".into()
    } else {
        stripped.into()
    })
}

fn normalize_route_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .strip_prefix("{*")
                .and_then(|value| value.strip_suffix('}'))
                .map(|value| format!("{{{value}}}"))
                .unwrap_or_else(|| segment.to_owned())
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn operation_id(method: &str, path: &str) -> String {
    let mut parts = vec![method.to_owned()];
    for segment in path.trim_matches('/').split('/') {
        if segment.is_empty() {
            continue;
        }
        if let Some(parameter) = segment
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            parts.push("by".into());
            parts.push(parameter.into());
        } else {
            parts.push(segment.replace('-', "_"));
        }
    }
    parts.join("_")
}

fn operation_tag(path: &str) -> &'static str {
    if path.starts_with("/health") || path == "/platform" {
        "Platform"
    } else if path.starts_with("/bootstrap") || path.contains("api-tokens") {
        "Identity"
    } else if path.starts_with("/node-control")
        || path.contains("/nodes")
        || path.contains("enrollment-tokens")
    {
        "Fleet"
    } else if path.contains("build-runs") {
        "Artifacts"
    } else if path.contains("source-") || path.starts_with("/webhooks") {
        "Sources"
    } else if path.contains("secrets") {
        "Secrets"
    } else if path.contains("routes") || path.contains("domain-claims") || path.contains("gateway-")
    {
        "Edge"
    } else if path.contains("workloads") || path.contains("deployments") {
        "Workloads"
    } else if path.contains("projects") || path.contains("environments") {
        "Projects"
    } else if path.contains("operations") {
        "Operations"
    } else if path.contains("search") {
        "Search"
    } else {
        "Organizations"
    }
}

fn requires_idempotency_key(method: &str, path: &str) -> bool {
    matches!(method, "delete" | "patch" | "post" | "put")
        && (path == "/bootstrap" || path == "/organizations" || path.starts_with("/organizations/"))
        && !path.ends_with("/source-connections/github")
}

fn accepts_acl(path: &str) -> bool {
    path.ends_with("/workloads") || (path.contains("/workloads/") && path.ends_with("/deployments"))
}

fn request_has_no_body(path: &str) -> bool {
    path.ends_with("/stop")
        || path.ends_with("/retry")
        || path.ends_with("/deactivate")
        || path.ends_with("/source-connections/github")
        || (path.contains("/secrets/") && path.ends_with("/revoke"))
}

fn asynchronous_mutation(path: &str) -> bool {
    path.contains("/deployments")
        || path.ends_with("/rollback")
        || path.ends_with("/stop")
        || path.ends_with("/retry")
        || path.ends_with("/verify")
        || (path.contains("domain-claims") && path.ends_with("/revoke"))
        || path.ends_with("/routes")
}

fn creates_resource(path: &str) -> bool {
    path == "/bootstrap"
        || path == "/node-control/enroll"
        || path == "/organizations"
        || path.ends_with("/projects")
        || path.ends_with("/environments")
        || path.ends_with("/api-tokens")
        || path.ends_with("/enrollment-tokens")
        || path.ends_with("/domain-claims")
        || path.ends_with("/gateway-scopes")
        || path.ends_with("/secrets")
        || path.ends_with("/versions")
        || path.ends_with("/source-revisions")
        || path.ends_with("/source-subscriptions/github")
        || path.ends_with("/source-connections/github")
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
