use super::components::response_ref;
use super::OPENAPI_CONTRACT_VERSION;
use crate::modules::applications::{
    APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES, APPLICATION_DESCRIPTION_MAX_CHARS,
    APPLICATION_INVOCATION_INPUT_MAX_BYTES, APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES,
    DEFAULT_APPLICATION_LIST_LIMIT, DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT,
    MAXIMUM_APPLICATION_LIST_LIMIT, MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
};
use crate::modules::connectors::{
    CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES, DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT,
    MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
};
use crate::modules::data::OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES;
use crate::modules::durable_cells::domain::{
    DURABLE_CELL_APPLICATION_MAX_ACL_BYTES, DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES,
    DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES,
};
use crate::modules::durable_cells::{
    DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT, MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
};
use crate::modules::forms::presentation::form_interaction_submission_schema;
use crate::modules::forms::CLOUD_FORM_DOCUMENT_MAX_BYTES;
use crate::modules::notifications::{
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
};
use crate::modules::projects::domain::value_objects::{
    BUSINESS_OWNER_REFERENCE_MAX_CHARS, COST_ATTRIBUTION_CODE_MAX_CHARS,
    PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS, PROJECT_ATTRIBUTION_LABEL_MAX_COUNT,
    PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS,
};
use crate::modules::workflow::{
    WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS, WORKFLOW_RUN_MAX_TIMEOUT_SECONDS,
};
use crate::modules::workloads::presentation::WORKLOAD_MANIFEST_MAX_BYTES;
use a3s_boot::{BootError, Result};
use a3s_use_extension::{
    plugin_catalog_host_input_schema, plugin_catalog_inspection_input_schema,
    plugin_catalog_search_input_schema,
};
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
    if let Some(description) = oidc_operation_description(method, path) {
        operation.insert("description".into(), json!(description));
        operation.insert("x-a3s-oauth-cookie-bound".into(), json!(true));
    }

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
        } else if parameter.get("name").and_then(Value::as_str) == Some("provider_key") {
            parameter.insert(
                "schema".into(),
                json!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 63,
                    "pattern": "^[a-z](?:[a-z0-9_-]{0,61}[a-z0-9])?$"
                }),
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
    if method == "post" && is_ontology_revision_mutation_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current Ontology aggregate version used for optimistic concurrency.",
                "schema": { "type": "integer", "minimum": 1 }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-migration-rule",
                "in": "header",
                "required": false,
                "description": "Target ACL migration-rule ID. Required only for a breaking structural diff.",
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 96,
                    "pattern": "^[A-Za-z0-9_-]+$"
                }
            }),
        );
    }
    if method == "post" && is_workflow_revision_mutation_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current WorkflowDefinition aggregate version used for optimistic concurrency.",
                "schema": { "type": "integer", "minimum": 1 }
            }),
        );
    }
    if method == "post" && is_human_task_assignment_mutation_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current HumanTask aggregate version used for optimistic concurrency.",
                "schema": { "type": "integer", "minimum": 1 }
            }),
        );
    }
    if method == "post" && is_form_version_mutation_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current Form draft aggregate version used for optimistic concurrency.",
                "schema": { "type": "integer", "minimum": 1 }
            }),
        );
    }
    if method == "post" && is_project_attribution_mutation_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current Project aggregate version used for optimistic concurrency.",
                "schema": { "type": "integer", "minimum": 1 }
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
    describe_query_parameters(parameters, method, path);
    Ok(())
}

fn describe_query_parameters(parameters: &mut Vec<Value>, method: &str, path: &str) {
    if is_asset_git_advertisement(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "service", "in": "query", "required": true,
                "schema": {
                    "type": "string",
                    "enum": ["git-upload-pack", "git-receive-pack"]
                }
            }),
        );
    }
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
    if path.ends_with("/release-selection") {
        upsert_parameter(
            parameters,
            json!({
                "name": "version", "in": "query", "required": false,
                "description": "Exact canonical semantic version. Omit to select the highest stable published release.",
                "schema": { "type": "string", "minLength": 1, "maxLength": 128 }
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
    if method == "get"
        && (path.ends_with("/operations")
            || path.ends_with("/audit-records")
            || path.ends_with("/build-runs")
            || path.ends_with("/agent-conversations")
            || path.ends_with("/executions")
            || path.ends_with("/workflow-runs")
            || path.ends_with("/human-tasks")
            || is_connector_profile_collection_path(path)
            || is_connector_revision_collection_path(path)
            || is_application_collection_path(path)
            || is_application_release_collection_path(path)
            || is_application_message_collection_path(path)
            || is_application_session_replay_path(path)
            || is_durable_cell_application_collection_path(path)
            || is_durable_cell_revision_collection_path(path))
    {
        let schema = if is_application_message_collection_path(path)
            || is_application_session_replay_path(path)
        {
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
                "default": DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT
            })
        } else if is_application_collection_path(path)
            || is_application_release_collection_path(path)
        {
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_APPLICATION_LIST_LIMIT
            })
        } else if is_durable_cell_application_collection_path(path)
            || is_durable_cell_revision_collection_path(path)
        {
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT
            })
        } else if path.ends_with("/audit-records")
            || is_connector_profile_collection_path(path)
            || is_connector_revision_collection_path(path)
        {
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
                "default": DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT
            })
        } else {
            json!({ "type": "integer", "minimum": 1, "maximum": 200 })
        };
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": schema
            }),
        );
    }
    if method == "get"
        && (is_application_message_collection_path(path)
            || is_application_session_replay_path(path))
    {
        upsert_parameter(
            parameters,
            json!({
                "name": "afterSequence",
                "in": "query",
                "required": false,
                "description": "Return messages strictly after this session sequence.",
                "schema": {"type": "integer", "minimum": 0, "default": 0}
            }),
        );
    }
    if method == "get" && path.ends_with("/audit-records") {
        for (name, format) in [
            ("actorPrincipalId", Some("uuid")),
            ("aggregateId", Some("uuid")),
            ("requestId", Some("uuid")),
            ("action", None),
            ("from", Some("date-time")),
            ("to", Some("date-time")),
            ("cursor", None),
        ] {
            let mut schema = json!({"type": "string", "minLength": 1});
            if let Some(format) = format {
                schema["format"] = json!(format);
            }
            if name == "action" {
                schema["maxLength"] = json!(255);
                schema["pattern"] = json!("^[a-z-]+(?:\\.[a-z-]+){2,}$");
            }
            if name == "cursor" {
                schema["maxLength"] = json!(128);
            }
            upsert_parameter(
                parameters,
                json!({"name": name, "in": "query", "required": false, "schema": schema}),
            );
        }
    }
    if method == "get" && path.ends_with("/notifications") {
        for parameter in [
            json!({
                "name": "unreadOnly", "in": "query", "required": false,
                "schema": { "type": "boolean", "default": false }
            }),
            json!({
                "name": "cursor", "in": "query", "required": false,
                "schema": { "type": "string", "minLength": 1, "maxLength": 128 }
            }),
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 200, "default": 50 }
            }),
        ] {
            upsert_parameter(parameters, parameter);
        }
    }
    if method == "get" && is_notification_outbound_subscription_collection_path(path) {
        for parameter in [
            json!({
                "name": "cursor", "in": "query", "required": false,
                "schema": { "type": "string", "minLength": 1, "maxLength": 128 }
            }),
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAXIMUM_NOTIFICATION_LIMIT,
                    "default": DEFAULT_NOTIFICATION_LIMIT
                }
            }),
        ] {
            upsert_parameter(parameters, parameter);
        }
    }
    if method == "get" && path.ends_with("/human-tasks") {
        upsert_parameter(
            parameters,
            json!({
                "name": "status", "in": "query", "required": false,
                "schema": {
                    "type": "string",
                    "enum": [
                        "pending_activation", "ready", "claimed", "completed", "expired", "cancelled"
                    ]
                }
            }),
        );
    }
    if method == "get" && path.ends_with("/workflow-runs/{workflow_run_id}/wait") {
        upsert_parameter(
            parameters,
            json!({
                "name": "timeoutSeconds", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 0, "maximum": 30, "default": 30 }
            }),
        );
    }
    if method == "get" && path.ends_with("/workflow-runs/{workflow_run_id}/history") {
        upsert_parameter(
            parameters,
            json!({
                "name": "afterSequence", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 0, "default": 0 }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 100, "default": 100 }
            }),
        );
    }
    if method == "get" && (path.ends_with("/events") || path.ends_with("/events/stream")) {
        let maximum = if path.ends_with("/stream") { 16 } else { 200 };
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
    if path == "/identity/oidc/{provider_key}/login" {
        upsert_parameter(
            parameters,
            json!({
                "name": "organization_id",
                "in": "query",
                "required": true,
                "description": "Organization context for the short-lived interactive credential.",
                "schema": { "type": "string", "format": "uuid" }
            }),
        );
    }
    if path == "/identity/oidc/{provider_key}/callback" {
        for (name, required, schema) in [
            (
                "code",
                false,
                json!({ "type": "string", "minLength": 1, "maxLength": 2048 }),
            ),
            (
                "state",
                true,
                json!({ "type": "string", "minLength": 43, "maxLength": 43 }),
            ),
            (
                "error",
                false,
                json!({ "type": "string", "maxLength": 2048 }),
            ),
        ] {
            upsert_parameter(
                parameters,
                json!({
                    "name": name,
                    "in": "query",
                    "required": required,
                    "schema": schema
                }),
            );
        }
    }
}

fn describe_request_body(operation: &mut Map<String, Value>, method: &str, path: &str) {
    if method != "post" || request_has_no_body(path) {
        return;
    }
    if let Some(media_type) = asset_git_request_media_type(path) {
        let mut content = Map::new();
        content.insert(
            media_type.to_owned(),
            json!({ "schema": { "type": "string", "format": "binary" } }),
        );
        operation.insert(
            "requestBody".into(),
            json!({
                "required": true,
                "content": content
            }),
        );
        return;
    }
    let mut content = Map::new();
    if let Some(schema) = plugin_catalog_read_request_schema(path) {
        content.insert("application/json".into(), json!({ "schema": schema }));
    } else if is_membership_invitation_create_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["principalId", "role", "expiresAt"],
                    "properties": {
                        "principalId": {"type": "string", "format": "uuid"},
                        "role": {
                            "type": "string",
                            "enum": ["owner", "admin", "member", "restricted"]
                        },
                        "expiresAt": {"type": "string", "format": "date-time"}
                    }
                }
            }),
        );
    } else if is_membership_invitation_version_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["expectedVersion"],
                    "properties": {
                        "expectedVersion": {"type": "integer", "minimum": 1}
                    }
                }
            }),
        );
    } else if is_resource_grant_create_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["scope"],
                    "properties": {
                        "scope": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["kind", "projectId"],
                                    "properties": {
                                        "kind": {"type": "string", "enum": ["project"]},
                                        "projectId": {"type": "string", "format": "uuid"}
                                    }
                                },
                                {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["kind", "projectId", "environmentId"],
                                    "properties": {
                                        "kind": {"type": "string", "enum": ["environment"]},
                                        "projectId": {"type": "string", "format": "uuid"},
                                        "environmentId": {"type": "string", "format": "uuid"}
                                    }
                                },
                                {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["kind", "nodeId"],
                                    "properties": {
                                        "kind": {"type": "string", "enum": ["node"]},
                                        "nodeId": {"type": "string", "format": "uuid"}
                                    }
                                }
                            ],
                            "discriminator": {"propertyName": "kind"}
                        }
                    }
                }
            }),
        );
    } else if is_resource_grant_revocation_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["expectedVersion"],
                    "properties": {
                        "expectedVersion": {"type": "integer", "minimum": 1}
                    }
                }
            }),
        );
    } else if is_project_attribution_mutation_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["businessOwnerReference"],
                    "properties": {
                        "businessOwnerReference": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": BUSINESS_OWNER_REFERENCE_MAX_CHARS
                        },
                        "costAttributionCode": {
                            "type": "string",
                            "nullable": true,
                            "minLength": 1,
                            "maxLength": COST_ATTRIBUTION_CODE_MAX_CHARS
                        },
                        "labels": {
                            "type": "object",
                            "maxProperties": PROJECT_ATTRIBUTION_LABEL_MAX_COUNT,
                            "propertyNames": {
                                "maxLength": PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS,
                                "pattern": "^[a-z][a-z0-9._-]*$"
                            },
                            "additionalProperties": {
                                "type": "string",
                                "minLength": 1,
                                "maxLength": PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS
                            },
                            "default": {}
                        }
                    }
                }
            }),
        );
    } else if is_application_request_body_path(path) {
        content.insert(
            "application/json".into(),
            json!({"schema": application_request_schema(path)}),
        );
    } else if is_durable_cell_mutation_path(path) {
        content.insert(
            "application/json".into(),
            json!({"schema": durable_cell_request_schema(path)}),
        );
    } else if is_connector_profile_mutation_path(path) {
        let schema = if is_connector_revision_collection_path(path) {
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["expectedVersion", "definitionAcl"],
                "properties": {
                    "expectedVersion": {"type": "integer", "minimum": 1},
                    "definitionAcl": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
                    }
                }
            })
        } else {
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["name", "definitionAcl"],
                "properties": {
                    "name": {"type": "string", "minLength": 1, "maxLength": 63},
                    "definitionAcl": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
                    }
                }
            })
        };
        content.insert("application/json".into(), json!({"schema": schema}));
    } else if is_notification_outbound_subscription_collection_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES
                }
            }),
        );
    } else if is_notification_outbound_subscription_revoke_path(path)
        || is_notification_read_path(path)
    {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["expectedVersion"],
                    "properties": {
                        "expectedVersion": {"type": "integer", "minimum": 1}
                    }
                }
            }),
        );
    } else if is_node_pool_mutation_path(path) {
        content.insert(
            "application/json".into(),
            json!({ "schema": node_pool_request_schema(path) }),
        );
    } else if is_human_task_submission_path(path) {
        content.insert(
            "application/json".into(),
            json!({ "schema": form_interaction_submission_schema() }),
        );
    } else if is_workflow_run_start_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["workflowGoalId", "planRevisionId"],
                    "properties": {
                        "workflowGoalId": { "type": "string", "format": "uuid" },
                        "planRevisionId": { "type": "string", "format": "uuid" },
                        "timeoutSeconds": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 2592000,
                            "default": 86400
                        }
                    }
                }
            }),
        );
    } else if is_workflow_run_cancel_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "reason": { "type": "string", "minLength": 1, "maxLength": 4096 }
                    }
                }
            }),
        );
    } else if is_form_draft_mutation_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "document"],
                    "properties": {
                        "name": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 120
                        },
                        "description": {
                            "type": "string",
                            "maxLength": 4096,
                            "default": ""
                        },
                        "document": {
                            "type": "object",
                            "description": "A native A3S Form document. Canonicalization and semantic validation remain owned by A3S Form.",
                            "x-a3s-max-canonical-bytes": CLOUD_FORM_DOCUMENT_MAX_BYTES
                        }
                    }
                }
            }),
        );
    } else if is_workflow_goal_mutation_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 262144
                }
            }),
        );
    } else if is_workflow_definition_mutation_path(path) {
        content.insert(
            "application/json".into(),
            json!({
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["definitionAcl", "payloads"],
                    "properties": {
                        "definitionAcl": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 1048576
                        },
                        "payloads": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 2048,
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["kind", "acl"],
                                "properties": {
                                    "kind": {
                                        "type": "string",
                                        "enum": ["configuration", "data_schema", "policy"]
                                    },
                                    "acl": {
                                        "type": "string",
                                        "minLength": 1,
                                        "maxLength": 262144
                                    }
                                }
                            }
                        },
                        "semanticContracts": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": [
                                "descriptorBindingsAcl",
                                "descriptorRegistryAcl",
                                "variableContractAcl"
                            ],
                            "properties": {
                                "descriptorBindingsAcl": {
                                    "type": "string",
                                    "minLength": 1,
                                    "maxLength": 524288
                                },
                                "descriptorRegistryAcl": {
                                    "type": "string",
                                    "minLength": 1,
                                    "maxLength": 4194304
                                },
                                "variableContractAcl": {
                                    "type": "string",
                                    "minLength": 1,
                                    "maxLength": 2097152
                                },
                                "variableDefaultsAcl": {
                                    "type": "string",
                                    "minLength": 1,
                                    "maxLength": 2097152
                                },
                                "compositeRegionsAcl": {
                                    "type": "string",
                                    "minLength": 1,
                                    "maxLength": 524288
                                }
                            }
                        }
                    }
                }
            }),
        );
    } else if is_ontology_mutation_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 1048576
                }
            }),
        );
    } else if is_mcp_service_profile_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 65536
                }
            }),
        );
    } else if is_mcp_route_policy_mutation_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 524288
                }
            }),
        );
    } else {
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
        let component = if let Some(component) = asset_git_success_component(path) {
            component.to_owned()
        } else if path.ends_with("/stream") {
            "SseSuccess200".to_owned()
        } else if path == "/node-control/enroll" {
            format!("RawSuccess{status}")
        } else {
            format!("Success{status}")
        };
        responses.insert(status.to_string(), response_ref(&component));
    }
    let mut error_statuses = vec![400, 404, 409, 422, 429, 500, 503];
    if asset_git_request_media_type(path).is_some()
        || (method == "post"
            && (is_ontology_mutation_path(path)
                || is_workflow_mutation_path(path)
                || is_human_task_submission_path(path)
                || is_form_draft_mutation_path(path)
                || is_mcp_service_profile_path(path)
                || is_mcp_route_policy_mutation_path(path)
                || is_application_request_body_path(path)
                || is_durable_cell_mutation_path(path)
                || is_notification_outbound_subscription_collection_path(path)))
    {
        error_statuses.extend([413, 415]);
    }
    for status in error_statuses {
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
    if path == "/identity/oidc/{provider_key}/login" {
        return vec![303];
    }
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
    if method == "post" && is_durable_cell_state_mutation_path(path) {
        return vec![200];
    }
    if method == "post"
        && (is_application_mutation_path(path)
            || is_durable_cell_application_collection_path(path)
            || is_durable_cell_revision_collection_path(path)
            || is_durable_cell_deployment_path(path)
            || is_durable_cell_route_path(path))
    {
        return vec![200, 201];
    }
    if method == "delete"
        && (path.contains("/deployments/")
            || path.contains("/build-runs/")
            || path.ends_with("/bindings"))
    {
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
    } else if path.starts_with("/bootstrap")
        || path.contains("api-tokens")
        || path.contains("memberships")
        || path.contains("membership-invitations")
        || path.contains("resource-grants")
        || path.contains("/identity/oidc")
    {
        "Identity"
    } else if path.starts_with("/node-control")
        || path.contains("/nodes")
        || path.contains("/node-pools")
        || path.contains("enrollment-tokens")
    {
        "Fleet"
    } else if path.contains("build-runs") {
        "Artifacts"
    } else if path.contains("agent-conversations") || path.contains("agent-executions") {
        "Agents"
    } else if path.contains("/assets")
        && (path.ends_with("/workloads") || path.ends_with("/deployments"))
    {
        "Workloads"
    } else if path.contains("/assets") {
        "Assets"
    } else if path.contains("source-") || path.starts_with("/webhooks") {
        "Sources"
    } else if path.contains("secrets") {
        "Secrets"
    } else if path.contains("durable-cell-applications") {
        "Durable Cells"
    } else if path.contains("routes")
        || path.contains("domain-claims")
        || path.contains("gateway-")
        || path.contains("mcp-credentials")
        || path.contains("mcp-route-policies")
    {
        "Edge"
    } else if path.contains("workloads") || path.contains("deployments") {
        "Workloads"
    } else if path.contains("ontologies")
        || path.contains("workflow-")
        || path.contains("human-tasks")
    {
        "Workflow"
    } else if path.contains("/forms") {
        "Forms"
    } else if path.contains("connector-profiles") {
        "Connectors"
    } else if path.contains("/applications") {
        "Applications"
    } else if path.contains("projects") || path.contains("environments") {
        "Projects"
    } else if path.contains("operations") {
        "Operations"
    } else if path.contains("audit-records") {
        "Audit"
    } else if path.contains("notifications") || path.contains("notification-outbound-subscriptions")
    {
        "Notifications"
    } else if path.contains("plugin-registries") {
        "Plugins"
    } else if path.contains("search") {
        "Search"
    } else {
        "Organizations"
    }
}

fn requires_idempotency_key(method: &str, path: &str) -> bool {
    matches!(method, "delete" | "patch" | "post" | "put")
        && (path == "/bootstrap"
            || path == "/organizations"
            || path.starts_with("/organizations/")
            || path.ends_with("/membership-invitations/{invitation_id}/acceptance"))
        && !path.ends_with("/source-connections/github")
        && !path.ends_with("/identity/oidc/{provider_key}/link")
        && !is_human_task_submission_path(path)
        && !is_plugin_catalog_read_path(path)
        && !is_asset_git_path(path)
}

fn is_plugin_catalog_read_path(path: &str) -> bool {
    path.contains("/plugin-registries/{registry_id}/catalog/")
        && (path.ends_with("/search") || path.ends_with("/inspect"))
}

fn plugin_catalog_read_request_schema(path: &str) -> Option<Value> {
    if !is_plugin_catalog_read_path(path) {
        return None;
    }
    let host = plugin_catalog_host_input_schema();
    if path.ends_with("/search") {
        return Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["host", "search"],
            "properties": {
                "host": host,
                "search": plugin_catalog_search_input_schema()
            }
        }));
    }

    let mut inspection = plugin_catalog_inspection_input_schema();
    let object = inspection.as_object_mut()?;
    object
        .get_mut("properties")?
        .as_object_mut()?
        .insert("host".into(), host);
    object
        .get_mut("required")?
        .as_array_mut()?
        .insert(0, json!("host"));
    Some(inspection)
}

fn accepts_acl(path: &str) -> bool {
    path.ends_with("/workloads") || (path.contains("/workloads/") && path.ends_with("/deployments"))
}

fn request_has_no_body(path: &str) -> bool {
    (path.ends_with("/stop") && !is_durable_cell_state_mutation_path(path))
        || path.ends_with("/retry")
        || path.ends_with("/archive")
        || path.ends_with("/yank")
        || path.ends_with("/deactivate")
        || path.ends_with("/bindings")
        || path.ends_with("/agent-conversations")
        || (path.contains("/agent-executions/") && path.ends_with("/cancel"))
        || path.ends_with("/source-connections/github")
        || path.ends_with("/identity/oidc/{provider_key}/link")
        || is_form_release_mutation_path(path)
        || is_human_task_assignment_mutation_path(path)
        || (path.contains("/secrets/") && path.ends_with("/revoke"))
}

fn oidc_operation_description(method: &str, path: &str) -> Option<&'static str> {
    match (method, path) {
        ("get", "/identity/oidc/{provider_key}/login") => Some(
            "Starts a public OIDC login and redirects to the configured provider. State, nonce, and S256 PKCE bind the one-time flow; nonce and verifier are held only in Secure HttpOnly callback cookies.",
        ),
        ("post", "/organizations/{organization_id}/identity/oidc/{provider_key}/link") => Some(
            "Starts an authenticated human-principal OIDC link flow. Returns the provider authorization URL and sets Secure HttpOnly callback cookies; the caller should navigate the browser to authorizationUrl.",
        ),
        ("get", "/identity/oidc/{provider_key}/callback") => Some(
            "Completes one OIDC login or link flow using the query state and callback-only HttpOnly cookies. Login credentials are returned once in JSON and never placed in a redirect URL.",
        ),
        _ => None,
    }
}

fn asynchronous_mutation(path: &str) -> bool {
    path.ends_with("/workloads")
        || path.contains("/deployments")
        || path.ends_with("/rollback")
        || path.ends_with("/bindings")
        || path.ends_with("/stop")
        || path.ends_with("/retry")
        || path.ends_with("/verify")
        || (path.contains("/agent-executions/") && path.ends_with("/cancel"))
        || (path.contains("domain-claims") && path.ends_with("/revoke"))
        || path.ends_with("/routes")
        || (path.contains("/agent-conversations/") && path.ends_with("/executions"))
        || is_workflow_run_start_path(path)
        || is_workflow_run_cancel_path(path)
}

fn creates_resource(path: &str) -> bool {
    path == "/bootstrap"
        || path == "/node-control/enroll"
        || path == "/organizations"
        || path.ends_with("/projects")
        || is_project_attribution_mutation_path(path)
        || path.ends_with("/environments")
        || path.ends_with("/ontologies")
        || is_ontology_revision_mutation_path(path)
        || path.ends_with("/workflow-definitions")
        || is_workflow_revision_mutation_path(path)
        || path.ends_with("/workflow-goals")
        || is_form_mutation_path(path)
        || path.ends_with("/api-tokens")
        || path.ends_with("/memberships")
        || path.ends_with("/membership-invitations")
        || path.ends_with("/membership-invitations/{invitation_id}/acceptance")
        || is_resource_grant_create_path(path)
        || path.ends_with("/enrollment-tokens")
        || path.ends_with("/node-pools")
        || path.ends_with("/domain-claims")
        || path.ends_with("/gateway-scopes")
        || path.ends_with("/mcp-credentials")
        || (path.contains("/mcp-credentials/") && path.ends_with("/rotate"))
        || path.ends_with("/mcp-route-policies")
        || (path.contains("/mcp-route-policies/") && path.ends_with("/revisions"))
        || is_connector_profile_mutation_path(path)
        || is_application_mutation_path(path)
        || is_durable_cell_application_collection_path(path)
        || is_durable_cell_revision_collection_path(path)
        || is_notification_outbound_subscription_collection_path(path)
        || path.ends_with("/secrets")
        || path.ends_with("/versions")
        || path.ends_with("/source-revisions")
        || path.ends_with("/source-subscriptions/github")
        || path.ends_with("/source-connections/github")
        || path.ends_with("/assets")
        || path.ends_with("/releases")
        || is_mcp_service_profile_path(path)
        || path.ends_with("/agent-conversations")
}

fn is_project_attribution_mutation_path(path: &str) -> bool {
    path.ends_with("/projects/{project_id}/attribution-profiles")
}

fn is_resource_grant_create_path(path: &str) -> bool {
    path.ends_with("/memberships/{membership_id}/resource-grants")
}

fn is_membership_invitation_create_path(path: &str) -> bool {
    path.starts_with("/organizations/") && path.ends_with("/membership-invitations")
}

fn is_membership_invitation_version_path(path: &str) -> bool {
    path.ends_with("/membership-invitations/{invitation_id}/acceptance")
        || path.ends_with("/membership-invitations/{invitation_id}/revocation")
}

fn is_notification_read_path(path: &str) -> bool {
    path.ends_with("/notifications/{notification_id}/read")
}

fn is_notification_outbound_subscription_collection_path(path: &str) -> bool {
    path.ends_with("/notification-outbound-subscriptions")
}

fn is_notification_outbound_subscription_revoke_path(path: &str) -> bool {
    path.ends_with("/notification-outbound-subscriptions/{subscription_id}/revoke")
}

fn is_connector_profile_mutation_path(path: &str) -> bool {
    is_connector_profile_collection_path(path) || is_connector_revision_collection_path(path)
}

fn is_connector_profile_collection_path(path: &str) -> bool {
    path.ends_with("/connector-profiles")
}

fn is_connector_revision_collection_path(path: &str) -> bool {
    path.contains("/connector-profiles/{profile_id}/") && path.ends_with("/revisions")
}

fn is_application_mutation_path(path: &str) -> bool {
    is_application_collection_path(path)
        || is_application_release_collection_path(path)
        || is_application_session_collection_path(path)
        || is_application_invocation_collection_path(path)
}

fn is_application_request_body_path(path: &str) -> bool {
    is_application_mutation_path(path)
        || is_application_session_close_path(path)
        || is_application_invocation_cancel_path(path)
}

fn is_application_collection_path(path: &str) -> bool {
    path.ends_with("/applications")
}

fn is_application_release_collection_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/") && path.ends_with("/releases")
}

fn is_application_session_collection_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/") && path.ends_with("/sessions")
}

fn is_application_invocation_collection_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/sessions/{session_id}/")
        && path.ends_with("/invocations")
}

fn is_application_message_collection_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/sessions/{session_id}/")
        && path.ends_with("/messages")
}

fn is_application_session_close_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/sessions/{session_id}/")
        && path.ends_with("/close")
}

fn is_application_session_replay_path(path: &str) -> bool {
    path.contains("/applications/{application_id}/sessions/{session_id}/")
        && path.ends_with("/replay")
}

fn is_application_invocation_cancel_path(path: &str) -> bool {
    path.contains(
        "/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/",
    ) && path.ends_with("/cancel")
}

fn application_request_schema(path: &str) -> Value {
    let release_acl = json!({
        "type": "string",
        "minLength": 1,
        "maxLength": APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES
    });
    if is_application_release_collection_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion", "releaseAcl"],
            "properties": {
                "expectedVersion": {"type": "integer", "minimum": 1},
                "releaseAcl": release_acl
            }
        });
    }
    if is_application_session_close_path(path) || is_application_invocation_cancel_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion"],
            "properties": {
                "expectedVersion": {"type": "integer", "minimum": 1}
            }
        });
    }
    if is_application_session_collection_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["releaseId"],
            "properties": {
                "releaseId": {"type": "string", "format": "uuid"},
                "initialVariables": {
                    "type": "object",
                    "x-a3s-max-canonical-bytes": APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES,
                    "default": {}
                }
            }
        });
    }
    if is_application_invocation_collection_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "ontologyId",
                "ontologyRevisionId",
                "responseMode",
                "input"
            ],
            "properties": {
                "ontologyId": {"type": "string", "format": "uuid"},
                "ontologyRevisionId": {"type": "string", "format": "uuid"},
                "environmentId": {
                    "type": "string",
                    "format": "uuid",
                    "nullable": true
                },
                "responseMode": {
                    "type": "string",
                    "enum": ["asynchronous", "blocking", "streaming"]
                },
                "input": {
                    "type": "object",
                    "x-a3s-max-canonical-bytes": APPLICATION_INVOCATION_INPUT_MAX_BYTES
                },
                "timeoutSeconds": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": WORKFLOW_RUN_MAX_TIMEOUT_SECONDS,
                    "default": WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS
                }
            }
        });
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "releaseAcl"],
        "properties": {
            "name": {"type": "string", "minLength": 1, "maxLength": 63},
            "description": {
                "type": "string",
                "maxLength": APPLICATION_DESCRIPTION_MAX_CHARS,
                "default": ""
            },
            "releaseAcl": release_acl
        }
    })
}

fn is_durable_cell_mutation_path(path: &str) -> bool {
    is_durable_cell_application_collection_path(path)
        || is_durable_cell_revision_collection_path(path)
        || is_durable_cell_state_mutation_path(path)
        || is_durable_cell_deployment_path(path)
        || is_durable_cell_route_path(path)
}

fn is_durable_cell_application_collection_path(path: &str) -> bool {
    path.ends_with("/durable-cell-applications")
}

fn is_durable_cell_revision_collection_path(path: &str) -> bool {
    path.contains("/durable-cell-applications/{application_id}/") && path.ends_with("/revisions")
}

fn is_durable_cell_state_mutation_path(path: &str) -> bool {
    path.contains("/durable-cell-applications/{application_id}/")
        && (path.ends_with("/start") || path.ends_with("/stop"))
}

fn is_durable_cell_deployment_path(path: &str) -> bool {
    path.contains("/durable-cell-applications/{application_id}/revisions/{revision_id}/deployments")
}

fn is_durable_cell_route_path(path: &str) -> bool {
    path.contains("/durable-cell-applications/{application_id}/revisions/{revision_id}/routes")
}

fn durable_cell_request_schema(path: &str) -> Value {
    let acl = |maximum: usize| {
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": maximum
        })
    };
    if is_durable_cell_application_collection_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "definitionAcl"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 63},
                "definitionAcl": acl(DURABLE_CELL_APPLICATION_MAX_ACL_BYTES)
            }
        });
    }
    if is_durable_cell_revision_collection_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion", "definitionAcl"],
            "properties": {
                "expectedVersion": {"type": "integer", "minimum": 1},
                "definitionAcl": acl(DURABLE_CELL_APPLICATION_MAX_ACL_BYTES)
            }
        });
    }
    if is_durable_cell_state_mutation_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion"],
            "properties": {
                "expectedVersion": {"type": "integer", "minimum": 1}
            }
        });
    }
    if is_durable_cell_deployment_path(path) {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "serviceProfileAcl", "providerWorkloadAcl", "storageBindingAcl"
            ],
            "properties": {
                "serviceProfileAcl": acl(DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES),
                "storageProviderProfileAcl": acl(OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES),
                "providerWorkloadAcl": acl(WORKLOAD_MANIFEST_MAX_BYTES),
                "storageBindingAcl": acl(DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES)
            }
        });
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "serviceProfileAcl", "gatewayScopeId", "domainClaimId", "hostname", "pathPrefix"
        ],
        "properties": {
            "serviceProfileAcl": acl(DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES),
            "gatewayScopeId": {"type": "string", "format": "uuid"},
            "domainClaimId": {"type": "string", "format": "uuid"},
            "hostname": {"type": "string", "minLength": 1, "maxLength": 253},
            "pathPrefix": {"type": "string", "minLength": 1, "maxLength": 2048}
        }
    })
}

fn is_resource_grant_revocation_path(path: &str) -> bool {
    path.ends_with("/resource-grants/{resource_grant_id}/revocation")
}

fn is_node_pool_mutation_path(path: &str) -> bool {
    path.ends_with("/node-pools")
        || path.ends_with("/node-pools/{node_pool_id}/members")
        || path.ends_with("/node-pools/{node_pool_id}/members/removal")
        || path.ends_with("/node-pools/{node_pool_id}/maintenance")
        || path.ends_with("/node-pools/{node_pool_id}/maintenance/cancel")
}

fn node_pool_request_schema(path: &str) -> Value {
    let expected_version = json!({ "type": "integer", "minimum": 1 });
    let node_ids = json!({
        "type": "array",
        "minItems": 1,
        "maxItems": 10000,
        "uniqueItems": true,
        "items": { "type": "string", "format": "uuid" }
    });
    if path.ends_with("/members") || path.ends_with("/members/removal") {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion", "memberNodeIds"],
            "properties": {
                "expectedVersion": expected_version,
                "memberNodeIds": node_ids
            }
        });
    }
    if path.ends_with("/maintenance/cancel") {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion", "maintenanceGeneration"],
            "properties": {
                "expectedVersion": expected_version,
                "maintenanceGeneration": { "type": "integer", "minimum": 1 }
            }
        });
    }
    if path.ends_with("/maintenance") {
        return json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedVersion", "targetNodeIds", "startsAt", "endsAt", "reason"],
            "properties": {
                "expectedVersion": expected_version,
                "targetNodeIds": node_ids,
                "startsAt": { "type": "string", "format": "date-time" },
                "endsAt": { "type": "string", "format": "date-time" },
                "reason": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }
        });
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "memberNodeIds"],
        "properties": {
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "memberNodeIds": node_ids
        }
    })
}

fn is_mcp_service_profile_path(path: &str) -> bool {
    path.ends_with("/mcp-service-profile")
        && path.contains("/assets/{asset_id}/releases/{asset_release_id}/")
}

fn is_ontology_mutation_path(path: &str) -> bool {
    path.ends_with("/ontologies") || is_ontology_revision_mutation_path(path)
}

fn is_ontology_revision_mutation_path(path: &str) -> bool {
    path.contains("/ontologies/{ontology_id}/") && path.ends_with("/revisions")
}

fn is_workflow_mutation_path(path: &str) -> bool {
    is_workflow_definition_mutation_path(path)
        || is_workflow_goal_mutation_path(path)
        || is_workflow_run_start_path(path)
        || is_workflow_run_cancel_path(path)
}

fn is_workflow_run_start_path(path: &str) -> bool {
    path.contains("/projects/{project_id}/") && path.ends_with("/workflow-runs")
}

fn is_workflow_run_cancel_path(path: &str) -> bool {
    path.contains("/workflow-runs/{workflow_run_id}/") && path.ends_with("/cancel")
}

fn is_human_task_assignment_mutation_path(path: &str) -> bool {
    path.contains("/human-tasks/{human_task_id}/")
        && (path.ends_with("/claim") || path.ends_with("/release"))
}

fn is_human_task_submission_path(path: &str) -> bool {
    path.contains("/human-tasks/{human_task_id}/") && path.ends_with("/submission")
}

fn is_workflow_definition_mutation_path(path: &str) -> bool {
    path.ends_with("/workflow-definitions") || is_workflow_revision_mutation_path(path)
}

fn is_workflow_revision_mutation_path(path: &str) -> bool {
    path.contains("/workflow-definitions/{workflow_definition_id}/") && path.ends_with("/revisions")
}

fn is_workflow_goal_mutation_path(path: &str) -> bool {
    path.ends_with("/workflow-goals")
}

fn is_form_mutation_path(path: &str) -> bool {
    is_form_draft_mutation_path(path) || is_form_release_mutation_path(path)
}

fn is_form_draft_mutation_path(path: &str) -> bool {
    path.ends_with("/forms")
        || (path.contains("/forms/{form_id}/") && path.ends_with("/draft-revisions"))
}

fn is_form_release_mutation_path(path: &str) -> bool {
    path.contains("/forms/{form_id}/") && path.ends_with("/releases")
}

fn is_form_version_mutation_path(path: &str) -> bool {
    (path.contains("/forms/{form_id}/") && path.ends_with("/draft-revisions"))
        || is_form_release_mutation_path(path)
}

fn is_mcp_route_policy_mutation_path(path: &str) -> bool {
    path.ends_with("/mcp-route-policies")
        || (path.contains("/mcp-route-policies/") && path.ends_with("/revisions"))
}

fn is_asset_git_path(path: &str) -> bool {
    path.contains("/assets/{asset_id}/git/")
}

fn is_asset_git_advertisement(path: &str) -> bool {
    is_asset_git_path(path) && path.ends_with("/info/refs")
}

fn asset_git_request_media_type(path: &str) -> Option<&'static str> {
    if !is_asset_git_path(path) {
        return None;
    }
    if path.ends_with("/git-upload-pack") {
        Some("application/x-git-upload-pack-request")
    } else if path.ends_with("/git-receive-pack") {
        Some("application/x-git-receive-pack-request")
    } else {
        None
    }
}

fn asset_git_success_component(path: &str) -> Option<&'static str> {
    if is_asset_git_advertisement(path) {
        Some("AssetGitAdvertisementSuccess200")
    } else if is_asset_git_path(path) && path.ends_with("/git-upload-pack") {
        Some("AssetGitUploadPackSuccess200")
    } else if is_asset_git_path(path) && path.ends_with("/git-receive-pack") {
        Some("AssetGitReceivePackSuccess200")
    } else {
        None
    }
}
