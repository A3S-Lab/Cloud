use super::components::response_ref;
use super::OPENAPI_CONTRACT_VERSION;
use a3s_boot::{BootError, Result};
use serde_json::{json, Map, Value};

pub(super) fn describe_operation(
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
    let json_schema = if path.contains("/mcp-credentials")
        && (path.ends_with("/mcp-credentials") || path.ends_with("/rotate"))
    {
        json!({ "$ref": "#/components/schemas/McpCredentialExpiryRequest" })
    } else {
        json!({ "type": "object", "additionalProperties": true })
    };
    content.insert("application/json".into(), json!({ "schema": json_schema }));
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
    let sensitive = path.contains("/mcp-credentials");
    for status in success_statuses(method, path) {
        let component = if path.ends_with("/stream") {
            "SseSuccess200".to_owned()
        } else if path == "/node-control/enroll" {
            format!("RawSuccess{status}")
        } else if sensitive {
            mcp_credential_success_component(method, path, status)
        } else {
            format!("Success{status}")
        };
        responses.insert(status.to_string(), response_ref(&component));
    }
    for status in [400, 404, 409, 422, 429, 500, 503] {
        let component = if sensitive {
            format!("SensitiveError{status}")
        } else {
            format!("Error{status}")
        };
        responses.insert(status.to_string(), response_ref(&component));
    }
    if !is_public || path == "/webhooks/github" {
        responses.insert(
            "401".into(),
            response_ref(if sensitive {
                "SensitiveError401"
            } else {
                "Error401"
            }),
        );
    }
    if !is_public {
        responses.insert(
            "403".into(),
            response_ref(if sensitive {
                "SensitiveError403"
            } else {
                "Error403"
            }),
        );
    }
    Value::Object(responses)
}

fn mcp_credential_success_component(method: &str, path: &str, status: u16) -> String {
    if method == "get" && path.ends_with("/mcp-credentials") {
        return "SensitiveMcpCredentialListSuccess200".into();
    }
    if method == "get" {
        return "SensitiveMcpCredentialSuccess200".into();
    }
    if method == "delete" {
        return "SensitiveMcpCredentialMutationSuccess200".into();
    }
    format!("SensitiveMcpCredentialDeliverySuccess{status}")
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
    } else if path.contains("routes")
        || path.contains("domain-claims")
        || path.contains("gateway-")
        || path.contains("mcp-credentials")
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
        || path.ends_with("/mcp-credentials")
        || path.ends_with("/secrets")
        || path.ends_with("/versions")
        || path.ends_with("/source-revisions")
        || path.ends_with("/source-subscriptions/github")
        || path.ends_with("/source-connections/github")
}
