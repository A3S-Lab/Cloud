use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub(super) fn example_from_schema(schema: &Value, field_name: Option<&str>) -> Value {
    if let Some(example) = schema.get("example") {
        return example.clone();
    }
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    if let Some(value) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return value.clone();
    }
    if let Some(value) = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return example_from_schema(value, field_name);
    }
    if let Some(value) = schema
        .get("anyOf")
        .and_then(Value::as_array)
        .and_then(|values| {
            values
                .iter()
                .find(|value| value.get("type") != Some(&json!("null")))
        })
    {
        return example_from_schema(value, field_name);
    }

    match schema.get("type").and_then(Value::as_str) {
        Some("object") | None if schema.get("properties").is_some() => {
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            let mut example = Map::new();
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, property) in properties {
                    if required.contains(name.as_str())
                        || property.get("default").is_some()
                        || example.len() < 4
                    {
                        example.insert(name.clone(), example_from_schema(property, Some(name)));
                    }
                }
            }
            Value::Object(example)
        }
        Some("array") => Value::Array(vec![example_from_schema(
            schema.get("items").unwrap_or(&Value::Null),
            field_name,
        )]),
        Some("integer") => schema.get("minimum").cloned().unwrap_or_else(|| json!(1)),
        Some("number") => schema.get("minimum").cloned().unwrap_or_else(|| json!(1.0)),
        Some("boolean") => json!(false),
        Some("null") => Value::Null,
        Some("string") => json!(string_example(schema, field_name)),
        _ => json!({}),
    }
}

pub(super) fn component_example(
    schema: &Value,
    schemas: &Map<String, Value>,
    field_name: Option<&str>,
    depth: usize,
) -> Value {
    if depth >= 16 {
        return json!({});
    }
    if let Some(example) = schema.get("example") {
        return example.clone();
    }
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    if let Some(value) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return value.clone();
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(name) = reference.strip_prefix("#/components/schemas/") {
            return schemas.get(name).map_or_else(
                || json!({}),
                |resolved| component_example(resolved, schemas, field_name, depth + 1),
            );
        }
    }
    if let Some(variant) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return component_example(variant, schemas, field_name, depth + 1);
    }

    let mut result = match schema.get("type").and_then(Value::as_str) {
        Some("object") | None if schema.get("properties").is_some() => {
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            let mut example = Map::new();
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, property) in properties {
                    if required.contains(name.as_str())
                        || property.get("default").is_some()
                        || example.len() < 4
                    {
                        example.insert(
                            name.clone(),
                            component_example(property, schemas, Some(name), depth + 1),
                        );
                    }
                }
            }
            Value::Object(example)
        }
        Some("array") => Value::Array(vec![component_example(
            schema.get("items").unwrap_or(&Value::Null),
            schemas,
            field_name,
            depth + 1,
        )]),
        Some("integer") => schema.get("minimum").cloned().unwrap_or_else(|| json!(1)),
        Some("number") => schema.get("minimum").cloned().unwrap_or_else(|| json!(1.0)),
        Some("boolean") => json!(false),
        Some("null") => Value::Null,
        Some("string") => json!(string_example(schema, field_name)),
        _ => json!({}),
    };
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for part in all_of {
            merge_example(
                &mut result,
                component_example(part, schemas, field_name, depth + 1),
            );
        }
    }
    result
}

fn merge_example(target: &mut Value, source: Value) {
    match (target.as_object_mut(), source) {
        (Some(target), Value::Object(source)) => {
            for (name, value) in source {
                target.insert(name, value);
            }
        }
        (_, source) => *target = source,
    }
}

fn string_example(schema: &Value, field_name: Option<&str>) -> String {
    let name = field_name.unwrap_or_default();
    let lowercase_name = name.to_ascii_lowercase();
    let pattern = schema.get("pattern").and_then(Value::as_str);
    let mut value = if schema.get("format").and_then(Value::as_str) == Some("uuid") {
        "00000000-0000-4000-8000-000000000001".into()
    } else if schema.get("format").and_then(Value::as_str) == Some("date-time") {
        "2026-08-22T00:00:00Z".into()
    } else if schema.get("format").and_then(Value::as_str) == Some("uri")
        || lowercase_name.ends_with("url")
    {
        "https://example.com/resource".into()
    } else if pattern == Some("^a3s_[0-9a-f]{64}$") {
        format!("a3s_{}", "0".repeat(64))
    } else if pattern == Some("^a3sn_[0-9a-f]{64}$") {
        format!("a3sn_{}", "0".repeat(64))
    } else if pattern == Some("^[0-9a-f]{40}$") {
        "0".repeat(40)
    } else if lowercase_name.contains("digest") {
        format!("sha256:{}", "0".repeat(64))
    } else if lowercase_name.ends_with("acl") {
        "version = 1\n".into()
    } else {
        match lowercase_name.as_str() {
            "idempotency-key" => "docs-example-001".into(),
            "x-a3s-bootstrap-token" => "bootstrap-token-redacted-example-0001".into(),
            "x-github-event" => "push".into(),
            "x-github-delivery" => "00000000-0000-4000-8000-000000000002".into(),
            "x-hub-signature-256" => format!("sha256={}", "0".repeat(64)),
            "last-event-id" | "cursor" => "eyJzZXF1ZW5jZSI6MTAwfQ".into(),
            "provider_key" => "corporate".into(),
            "q" => "runtime".into(),
            "code" => "authorization-code".into(),
            "state" => "opaque-one-time-state".into(),
            "error" => "access_denied".into(),
            "action" => "cloud.resource.updated".into(),
            "stream" => "stdout".into(),
            "service" => "git-upload-pack".into(),
            "name" => "Example resource".into(),
            "reason" => "Operator requested cancellation".into(),
            "version" => "1.0.0".into(),
            "scopes" => "organization:read".into(),
            "pattern" | "hostname" => "app.example.com".into(),
            "path" | "pathprefix" => "/".into(),
            "portname" => "http".into(),
            "mediatype" => "application/vnd.oci.image.manifest.v1+json".into(),
            "uri" => "oci://registry.example.com/a3s/example:1.0.0".into(),
            "commitsha" | "after" => "0".repeat(40),
            "csr_pem" => "-----BEGIN CERTIFICATE REQUEST-----\nZXhhbXBsZQ==\n-----END CERTIFICATE REQUEST-----\n".into(),
            "full_name" => "A3S-Lab/Cloud".into(),
            "provider_id" => "a3s-box".into(),
            "provider_build" => "3.2.0".into(),
            "node_name" => "worker-01".into(),
            "agent_version" => "1.0.0".into(),
            "artifact_media_types" => "application/vnd.oci.image.manifest.v1+json".into(),
            "ref" => "refs/heads/main".into(),
            "contextpath" => ".".into(),
            "dockerfilepath" => "Dockerfile".into(),
            "value" if schema.get("writeOnly").and_then(Value::as_bool) == Some(true) => {
                "example-secret-value".into()
            }
            "value" => "main".into(),
            _ if lowercase_name.ends_with("_id") || lowercase_name.ends_with("id") => {
                "00000000-0000-4000-8000-000000000001".into()
            }
            _ => "example".into(),
        }
    };
    if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
        while value.chars().count() < minimum as usize {
            value.push('x');
        }
    }
    value
}
