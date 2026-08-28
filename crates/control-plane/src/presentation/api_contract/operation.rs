use super::components::response_ref;
use super::developer_workflow_operation::{
    is_build_plan_detection_path, is_developer_workflow_creation_path, is_developer_workflow_path,
    is_developer_workflow_request_body_path,
    path_parameter_schema as developer_workflow_path_parameter_schema,
    query_parameters as developer_workflow_query_parameters,
    success_component as developer_workflow_success_component,
};
use super::documentation::describe_operation_documentation;
use super::request_schema::closed_json_request_schema;
use super::source_discovery_operation::{
    query_parameters as source_discovery_query_parameters,
    success_component as source_discovery_success_component,
};
use super::OPENAPI_CONTRACT_VERSION;
use crate::modules::applications::{
    APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES, APPLICATION_DESCRIPTION_MAX_CHARS,
    APPLICATION_INVOCATION_INPUT_MAX_BYTES, APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES,
    DEFAULT_APPLICATION_LIST_LIMIT, DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT,
    MAXIMUM_APPLICATION_LIST_LIMIT, MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
};
use crate::modules::audit::{
    DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE, DEFAULT_AUDIT_RECORD_LIMIT, MAXIMUM_AUDIT_RECORD_LIMIT,
};
use crate::modules::connectors::{
    CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES,
    CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES, CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES,
    DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE, DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE, MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
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
    NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES, OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
};
use crate::modules::projects::domain::value_objects::{
    BUSINESS_OWNER_REFERENCE_MAX_CHARS, COST_ATTRIBUTION_CODE_MAX_CHARS,
    PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS, PROJECT_ATTRIBUTION_LABEL_MAX_COUNT,
    PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS,
};
use crate::modules::security::{DEFAULT_SECURITY_TIMELINE_LIMIT, MAXIMUM_SECURITY_TIMELINE_LIMIT};
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
    let tag = operation_tag(path);
    operation.insert("tags".into(), json!([tag]));
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
    if path.contains("/identity/oidc/") {
        operation.insert("x-a3s-oauth-cookie-bound".into(), json!(true));
    }

    describe_parameters(operation, method, path)?;
    describe_request_body(operation, method, path)?;
    operation.insert("responses".into(), responses(method, path, is_public));
    describe_operation_documentation(operation, method, path, tag, is_public)?;
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
        let specialized_schema = parameter
            .get("name")
            .and_then(Value::as_str)
            .and_then(|name| developer_workflow_path_parameter_schema(path, name));
        let is_identifier = parameter
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.ends_with("_id") && name != "unit_id");
        if let Some(schema) = specialized_schema {
            parameter.insert("schema".into(), schema);
        } else if is_identifier {
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
    if method == "post" && is_agent_approval_decision_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "x-a3s-expected-version",
                "in": "header",
                "required": true,
                "description": "Current Agent approval checkpoint version used for optimistic concurrency.",
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
    for parameter in developer_workflow_query_parameters(method, path) {
        upsert_parameter(parameters, parameter);
    }
    for parameter in source_discovery_query_parameters(method, path) {
        upsert_parameter(parameters, parameter);
    }
    let is_audit_export_manifest = path.ends_with("/audit-records/export/manifest");
    let is_audit_record_query =
        path.ends_with("/audit-records") || path.ends_with("/audit-records/export");
    let has_audit_filters = is_audit_record_query || is_audit_export_manifest;
    if method == "get" && is_agent_approval_collection_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "status", "in": "query", "required": false,
                "schema": {
                    "type": "string",
                    "enum": ["pending", "approved", "denied", "expired", "resumed", "cancelled"]
                }
            }),
        );
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 50 }
            }),
        );
    }
    if method == "get" && is_agent_execution_checkpoint_collection_path(path) {
        upsert_parameter(
            parameters,
            json!({
                "name": "limit", "in": "query", "required": false,
                "description": "Maximum immutable checkpoint projections to return in reverse trajectory order.",
                "schema": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 50 }
            }),
        );
    }
    if method == "get" && is_agent_execution_trajectory_path(path) {
        for parameter in [
            json!({
                "name": "cursor", "in": "query", "required": false,
                "description": "Opaque cursor for the last semantic event already consumed.",
                "schema": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
            json!({
                "name": "throughSequence", "in": "query", "required": false,
                "description": "Inclusive upper semantic event sequence boundary.",
                "schema": {
                    "type": "integer", "format": "int64", "minimum": 1,
                    "maximum": 9007199254740991_i64
                }
            }),
            json!({
                "name": "limit", "in": "query", "required": false,
                "schema": { "type": "integer", "minimum": 1, "maximum": 200, "default": 100 }
            }),
        ] {
            upsert_parameter(parameters, parameter);
        }
    }
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
            || is_audit_record_query
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
        } else if is_audit_record_query {
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_AUDIT_RECORD_LIMIT,
                "default": DEFAULT_AUDIT_RECORD_LIMIT
            })
        } else if is_connector_profile_collection_path(path)
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
    if method == "get" && has_audit_filters {
        for (name, format) in [
            ("actorPrincipalId", Some("uuid")),
            ("aggregateId", Some("uuid")),
            ("requestId", Some("uuid")),
            ("projectId", Some("uuid")),
            ("environmentId", Some("uuid")),
            ("attributionProfileId", Some("uuid")),
            ("attributionStatus", None),
            ("action", None),
            ("from", Some("date-time")),
            ("to", Some("date-time")),
            ("cursor", None),
        ] {
            if is_audit_export_manifest && name == "cursor" {
                continue;
            }
            let mut schema = json!({"type": "string", "minLength": 1});
            if let Some(format) = format {
                schema["format"] = json!(format);
            }
            if name == "action" {
                schema["maxLength"] = json!(255);
                schema["pattern"] = json!("^[a-z-]+(?:\\.[a-z-]+){2,}$");
            }
            if name == "attributionStatus" {
                schema["enum"] = json!([
                    "legacy_unknown",
                    "not_applicable",
                    "profile_missing",
                    "profile_bound"
                ]);
            }
            if name == "cursor" {
                schema["maxLength"] = json!(128);
            }
            upsert_parameter(
                parameters,
                json!({
                    "name": name,
                    "in": "query",
                    "required": (path.ends_with("/audit-records/export")
                        || is_audit_export_manifest)
                        && matches!(name, "from" | "to"),
                    "schema": schema
                }),
            );
        }
    }
    if method == "get" && is_audit_export_manifest {
        upsert_parameter(
            parameters,
            json!({
                "name": "pageSize", "in": "query", "required": false,
                "schema": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAXIMUM_AUDIT_RECORD_LIMIT,
                    "default": DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE
                }
            }),
        );
    }
    if method == "get" && is_security_gateway_route_policy_timeline_path(path) {
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
                    "maximum": MAXIMUM_SECURITY_TIMELINE_LIMIT,
                    "default": DEFAULT_SECURITY_TIMELINE_LIMIT
                }
            }),
        ] {
            upsert_parameter(parameters, parameter);
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
    if method == "get" && is_notification_alert_policy_collection_path(path) {
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
    if method == "get" && is_connector_execution_attempt_collection_path(path) {
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
                    "maximum": MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
                    "default": DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE
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

fn describe_request_body(
    operation: &mut Map<String, Value>,
    method: &str,
    path: &str,
) -> Result<()> {
    if method != "post" || request_has_no_body(path) {
        return Ok(());
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
        return Ok(());
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
        let schema = if is_connector_revision_revocation_path(path)
            || is_connector_execution_attempt_resolution_path(path)
        {
            let maximum = if is_connector_execution_attempt_resolution_path(path) {
                CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES
            } else {
                CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES
            };
            let example = if is_connector_execution_attempt_resolution_path(path) {
                "Provider outcome could not be established"
            } else {
                "Operator requested cancellation"
            };
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": maximum,
                        "pattern": "^[^\\u0000-\\u001F\\u007F-\\u009F]+$",
                        "example": example,
                        "x-a3s-max-utf8-bytes":
                            maximum
                    }
                }
            })
        } else if is_connector_revision_collection_path(path) {
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["expectedVersion", "definitionAcl"],
                "properties": {
                    "expectedVersion": {"type": "integer", "minimum": 1},
                    "definitionAcl": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES,
                        "x-a3s-max-canonical-bytes":
                            CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
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
                        "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES,
                        "x-a3s-max-canonical-bytes":
                            CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
                    }
                }
            })
        };
        content.insert("application/json".into(), json!({"schema": schema}));
    } else if is_notification_alert_policy_collection_path(path) {
        content.insert(
            "application/vnd.a3s.acl".into(),
            json!({
                "schema": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES
                }
            }),
        );
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
    } else if is_notification_alert_policy_revoke_path(path)
        || is_notification_outbound_subscription_revoke_path(path)
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
    } else if let Some(schema) = closed_json_request_schema(path) {
        content.insert("application/json".into(), json!({ "schema": schema }));
        if accepts_acl(path) {
            content.insert(
                "application/vnd.a3s.acl".into(),
                json!({ "schema": { "type": "string", "minLength": 1 } }),
            );
        }
    } else {
        return Err(BootError::Internal(format!(
            "OpenAPI request schema is missing for `POST {path}`"
        )));
    }
    operation.insert(
        "requestBody".into(),
        json!({
            "required": true,
            "content": content
        }),
    );
    Ok(())
}

fn responses(method: &str, path: &str, is_public: bool) -> Value {
    let mut responses = Map::new();
    for status in success_statuses(method, path) {
        let component = if is_security_gateway_route_policy_timeline_path(path) {
            "SecurityGatewayRoutePolicyTimelinePageSuccess200".to_owned()
        } else if let Some(component) = source_discovery_success_component(method, path, status) {
            component.to_owned()
        } else if let Some(component) = developer_workflow_success_component(method, path, status) {
            component
        } else if let Some(component) = recipient_contact_success_component(method, path, status) {
            component
        } else if let Some(component) =
            notification_alert_policy_success_component(method, path, status)
        {
            component
        } else if let Some(component) =
            notification_outbound_subscription_success_component(method, path, status)
        {
            component
        } else if let Some(component) = agent_success_component(method, path, status) {
            component
        } else if let Some(component) = workflow_success_component(method, path, status) {
            component
        } else if let Some(component) = connector_success_component(method, path, status) {
            component
        } else if let Some(component) = asset_git_success_component(path) {
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
                || is_connector_profile_mutation_path(path)
                || is_recipient_contact_mutation_path(path)
                || is_notification_alert_policy_collection_path(path)
                || is_notification_outbound_subscription_collection_path(path)
                || is_agent_approval_decision_path(path)
                || is_agent_execution_checkpoint_collection_path(path)
                || is_agent_execution_fork_path(path)
                || is_developer_workflow_request_body_path(path)))
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
    if method == "post" && is_recipient_contact_collection_path(path) {
        return vec![200, 202];
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
    } else if path.contains("security-investigations") {
        "Security"
    } else if path.starts_with("/bootstrap")
        || path.contains("api-tokens")
        || path.contains("memberships")
        || path.contains("membership-invitations")
        || path.contains("resource-grants")
        || path.contains("recipient-contacts")
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
    } else if is_developer_workflow_path(path) {
        "Developer Workflows"
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
    } else if path.contains("notifications")
        || path.contains("notification-alert-policies")
        || path.contains("notification-outbound-subscriptions")
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
        && !is_build_plan_detection_path(path)
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

fn asynchronous_mutation(path: &str) -> bool {
    path.ends_with("/workloads")
        || path.contains("/deployments")
        || path.ends_with("/rollback")
        || path.ends_with("/bindings")
        || path.ends_with("/stop")
        || path.ends_with("/retry")
        || path.ends_with("/verify")
        || is_recipient_contact_collection_path(path)
        || (path.contains("/agent-executions/") && path.ends_with("/cancel"))
        || is_agent_approval_decision_path(path)
        || is_agent_execution_fork_path(path)
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
        || is_notification_alert_policy_collection_path(path)
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
        || is_developer_workflow_creation_path(path)
        || is_agent_execution_checkpoint_collection_path(path)
}

fn is_recipient_contact_collection_path(path: &str) -> bool {
    path == "/organizations/{organization_id}/recipient-contacts"
}

fn is_recipient_contact_item_path(path: &str) -> bool {
    path == "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}"
}

fn is_recipient_contact_verification_path(path: &str) -> bool {
    path
        == "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/verification"
}

fn is_recipient_contact_revocation_path(path: &str) -> bool {
    path == "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/revocation"
}

fn is_recipient_contact_mutation_path(path: &str) -> bool {
    is_recipient_contact_collection_path(path)
        || is_recipient_contact_verification_path(path)
        || is_recipient_contact_revocation_path(path)
}

fn recipient_contact_success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "get" && is_recipient_contact_collection_path(path) {
        Some("RecipientContactListSuccess200".into())
    } else if method == "get" && is_recipient_contact_item_path(path) {
        Some("RecipientContactSuccess200".into())
    } else if method == "post" && is_recipient_contact_mutation_path(path) {
        Some(format!("RecipientContactMutationSuccess{status}"))
    } else {
        None
    }
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

fn is_security_gateway_route_policy_timeline_path(path: &str) -> bool {
    path.ends_with("/security-investigations/gateway-routes/{route_id}/timeline")
}

fn is_notification_alert_policy_collection_path(path: &str) -> bool {
    path.ends_with("/notification-alert-policies")
}

fn is_notification_alert_policy_revoke_path(path: &str) -> bool {
    path.ends_with("/notification-alert-policies/{policy_id}/revoke")
}

fn is_notification_alert_policy_item_path(path: &str) -> bool {
    path.ends_with("/notification-alert-policies/{policy_id}")
}

fn notification_alert_policy_success_component(
    method: &str,
    path: &str,
    status: u16,
) -> Option<String> {
    if method == "get" && is_notification_alert_policy_collection_path(path) {
        Some("NotificationAlertPolicyPageSuccess200".into())
    } else if method == "get" && is_notification_alert_policy_item_path(path) {
        Some("NotificationAlertPolicySuccess200".into())
    } else if method == "post"
        && (is_notification_alert_policy_collection_path(path)
            || is_notification_alert_policy_revoke_path(path))
    {
        Some(format!("NotificationAlertPolicyMutationSuccess{status}"))
    } else {
        None
    }
}

fn is_notification_outbound_subscription_collection_path(path: &str) -> bool {
    path.ends_with("/notification-outbound-subscriptions")
}

fn is_notification_outbound_subscription_revoke_path(path: &str) -> bool {
    path.ends_with("/notification-outbound-subscriptions/{subscription_id}/revoke")
}

fn is_notification_outbound_subscription_item_path(path: &str) -> bool {
    path.ends_with("/notification-outbound-subscriptions/{subscription_id}")
}

fn notification_outbound_subscription_success_component(
    method: &str,
    path: &str,
    status: u16,
) -> Option<String> {
    if method == "get" && is_notification_outbound_subscription_collection_path(path) {
        Some("OutboundNotificationSubscriptionPageSuccess200".into())
    } else if method == "get" && is_notification_outbound_subscription_item_path(path) {
        Some("OutboundNotificationSubscriptionSuccess200".into())
    } else if method == "post"
        && (is_notification_outbound_subscription_collection_path(path)
            || is_notification_outbound_subscription_revoke_path(path))
    {
        Some(format!(
            "OutboundNotificationSubscriptionMutationSuccess{status}"
        ))
    } else {
        None
    }
}

fn is_connector_profile_mutation_path(path: &str) -> bool {
    is_connector_profile_collection_path(path)
        || is_connector_revision_collection_path(path)
        || is_connector_revision_revocation_path(path)
        || is_connector_execution_attempt_resolution_path(path)
}

fn is_connector_profile_collection_path(path: &str) -> bool {
    path.ends_with("/connector-profiles")
}

fn is_connector_revision_collection_path(path: &str) -> bool {
    path.contains("/connector-profiles/{profile_id}/") && path.ends_with("/revisions")
}

fn is_connector_revision_item_path(path: &str) -> bool {
    path.contains("/connector-profiles/{profile_id}/revisions/{revision_id}")
        && path.ends_with("/{revision_id}")
}

fn is_connector_revision_revocation_path(path: &str) -> bool {
    path.ends_with("/connector-profiles/{profile_id}/revisions/{revision_id}/revocation")
}

fn is_connector_execution_attempt_collection_path(path: &str) -> bool {
    path.ends_with("/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts")
}

fn is_connector_execution_attempt_item_path(path: &str) -> bool {
    path.ends_with(
        "/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}",
    )
}

fn is_connector_execution_attempt_resolution_path(path: &str) -> bool {
    path.ends_with(
        "/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}/resolution",
    )
}

fn is_connector_profile_item_path(path: &str) -> bool {
    path.ends_with("/connector-profiles/{profile_id}")
}

fn connector_success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "get" && is_connector_profile_collection_path(path) {
        Some("ConnectorProfileListSuccess200".into())
    } else if method == "post" && is_connector_profile_collection_path(path) {
        Some(format!("ConnectorProfileMutationSuccess{status}"))
    } else if method == "get" && is_connector_profile_item_path(path) {
        Some("ConnectorProfileRecordSuccess200".into())
    } else if method == "get" && is_connector_revision_collection_path(path) {
        Some("ConnectorRevisionListSuccess200".into())
    } else if method == "post" && is_connector_revision_collection_path(path) {
        Some(format!("ConnectorProfileMutationSuccess{status}"))
    } else if method == "get" && is_connector_revision_item_path(path) {
        Some("ConnectorRevisionSuccess200".into())
    } else if method == "get" && is_connector_revision_revocation_path(path) {
        Some("ConnectorRevisionRevocationSuccess200".into())
    } else if method == "post" && is_connector_revision_revocation_path(path) {
        Some(format!(
            "ConnectorRevisionRevocationMutationSuccess{status}"
        ))
    } else if method == "get" && is_connector_execution_attempt_collection_path(path) {
        Some("ConnectorExecutionAttemptPageSuccess200".into())
    } else if method == "get" && is_connector_execution_attempt_item_path(path) {
        Some("ConnectorExecutionAttemptSuccess200".into())
    } else if method == "get" && is_connector_execution_attempt_resolution_path(path) {
        Some("ConnectorExecutionAttemptResolutionSuccess200".into())
    } else if method == "post" && is_connector_execution_attempt_resolution_path(path) {
        Some(format!(
            "ConnectorExecutionAttemptResolutionMutationSuccess{status}"
        ))
    } else {
        None
    }
}

fn is_application_mutation_path(path: &str) -> bool {
    is_application_collection_path(path)
        || is_application_release_collection_path(path)
        || is_application_session_collection_path(path)
        || is_application_invocation_collection_path(path)
}

fn agent_success_component(method: &str, path: &str, status: u16) -> Option<String> {
    let conversation_collection =
        path.ends_with("/projects/{project_id}/environments/{environment_id}/agent-conversations");
    let conversation_item = path.ends_with("/agent-conversations/{conversation_id}");
    let execution_collection = path.ends_with("/agent-conversations/{conversation_id}/executions");
    let execution_item = path.ends_with("/agent-executions/{execution_id}");
    let execution_cancel = path.ends_with("/agent-executions/{execution_id}/cancel");
    let execution_change_set = path.ends_with("/agent-executions/{execution_id}/changes");
    let approval_collection = is_agent_approval_collection_path(path);
    let approval_item = is_agent_approval_item_path(path);
    let approval_decision = is_agent_approval_decision_path(path);
    let checkpoint_collection = is_agent_execution_checkpoint_collection_path(path);
    let checkpoint_item = is_agent_execution_checkpoint_item_path(path);
    let checkpoint_snapshot = is_agent_execution_checkpoint_snapshot_path(path);
    let execution_fork = is_agent_execution_fork_path(path);
    let execution_trajectory = is_agent_execution_trajectory_path(path);
    let event_page = path.ends_with("/agent-conversations/{conversation_id}/events");

    if method == "get" && conversation_collection {
        Some("AgentConversationListSuccess200".into())
    } else if method == "post" && conversation_collection {
        Some(format!("AgentConversationMutationSuccess{status}"))
    } else if method == "get" && conversation_item {
        Some("AgentConversationSuccess200".into())
    } else if method == "get" && execution_collection {
        Some("AgentExecutionListSuccess200".into())
    } else if method == "post" && execution_collection {
        Some(format!("AgentExecutionMutationSuccess{status}"))
    } else if method == "get" && execution_item {
        Some("AgentExecutionSuccess200".into())
    } else if method == "post" && execution_cancel {
        Some(format!("AgentExecutionMutationSuccess{status}"))
    } else if method == "get" && execution_change_set {
        Some("AgentExecutionChangeSetSuccess200".into())
    } else if method == "get" && checkpoint_collection {
        Some("AgentExecutionCheckpointListSuccess200".into())
    } else if method == "post" && checkpoint_collection {
        Some(format!("AgentExecutionCheckpointMutationSuccess{status}"))
    } else if method == "get" && checkpoint_item {
        Some("AgentExecutionCheckpointSuccess200".into())
    } else if method == "get" && checkpoint_snapshot {
        Some("AgentExecutionCheckpointSnapshotSuccess200".into())
    } else if method == "post" && execution_fork {
        Some(format!("AgentExecutionMutationSuccess{status}"))
    } else if method == "get" && execution_trajectory {
        Some("AgentExecutionTrajectoryPageSuccess200".into())
    } else if method == "get" && approval_collection {
        Some("AgentApprovalCheckpointListSuccess200".into())
    } else if method == "get" && approval_item {
        Some("AgentApprovalCheckpointSuccess200".into())
    } else if method == "post" && approval_decision {
        Some(format!("AgentApprovalCheckpointMutationSuccess{status}"))
    } else if method == "get" && event_page {
        Some("AgentExecutionEventPageSuccess200".into())
    } else {
        None
    }
}

fn is_agent_approval_collection_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/approval-checkpoints")
}

fn is_agent_approval_item_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/approval-checkpoints/{checkpoint_id}")
}

fn is_agent_approval_decision_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/approval-checkpoints/{checkpoint_id}/decision")
}

fn is_agent_execution_checkpoint_collection_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/checkpoints")
}

fn is_agent_execution_checkpoint_item_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/checkpoints/{checkpoint_id}")
}

fn is_agent_execution_checkpoint_snapshot_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/checkpoints/{checkpoint_id}/snapshot")
}

fn is_agent_execution_fork_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/checkpoints/{checkpoint_id}/fork")
}

fn is_agent_execution_trajectory_path(path: &str) -> bool {
    path.ends_with("/agent-executions/{execution_id}/trajectory")
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

fn is_workflow_definition_collection_path(path: &str) -> bool {
    path.contains("/projects/{project_id}/") && path.ends_with("/workflow-definitions")
}

fn is_workflow_definition_item_path(path: &str) -> bool {
    path.ends_with("/workflow-definitions/{workflow_definition_id}")
}

fn is_workflow_revision_mutation_path(path: &str) -> bool {
    path.contains("/workflow-definitions/{workflow_definition_id}/") && path.ends_with("/revisions")
}

fn is_workflow_revision_item_path(path: &str) -> bool {
    path.contains("/workflow-definitions/{workflow_definition_id}/")
        && path.ends_with("/revisions/{workflow_revision_id}")
}

fn workflow_success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "post" && is_ontology_mutation_path(path) {
        Some(format!("OntologyMutationSuccess{status}"))
    } else if method == "get" && is_ontology_collection_path(path) {
        Some("OntologyListSuccess200".into())
    } else if method == "get" && is_ontology_item_path(path) {
        Some("OntologySuccess200".into())
    } else if method == "get" && is_ontology_revision_mutation_path(path) {
        Some("OntologyRevisionSummaryListSuccess200".into())
    } else if method == "get" && is_ontology_revision_item_path(path) {
        Some("OntologyRevisionSuccess200".into())
    } else if method == "get" && is_ontology_diff_path(path) {
        Some("OntologyDiffSuccess200".into())
    } else if method == "post" && is_human_task_mutation_path(path) {
        Some(format!("HumanTaskMutationSuccess{status}"))
    } else if method == "get" && is_human_task_collection_path(path) {
        Some("HumanTaskListSuccess200".into())
    } else if method == "get" && is_human_task_item_path(path) {
        Some("HumanTaskSuccess200".into())
    } else if method == "post" && is_workflow_definition_mutation_path(path) {
        Some(format!("WorkflowDefinitionMutationSuccess{status}"))
    } else if method == "post" && is_workflow_goal_mutation_path(path) {
        Some(format!("WorkflowGoalMutationSuccess{status}"))
    } else if method == "post"
        && (is_workflow_run_start_path(path) || is_workflow_run_cancel_path(path))
    {
        Some(format!("WorkflowRunMutationSuccess{status}"))
    } else if method == "get" && is_workflow_definition_collection_path(path) {
        Some("WorkflowDefinitionListSuccess200".into())
    } else if method == "get" && is_workflow_definition_item_path(path) {
        Some("WorkflowDefinitionSuccess200".into())
    } else if method == "get" && is_workflow_revision_mutation_path(path) {
        Some("WorkflowRevisionSummaryListSuccess200".into())
    } else if method == "get" && is_workflow_revision_item_path(path) {
        Some("WorkflowRevisionSuccess200".into())
    } else if method == "get" && is_workflow_node_catalog_path(path) {
        Some("WorkflowNodeCatalogSuccess200".into())
    } else if method == "get" && is_workflow_goal_mutation_path(path) {
        Some("WorkflowGoalListSuccess200".into())
    } else if method == "get" && is_workflow_goal_item_path(path) {
        Some("WorkflowGoalSuccess200".into())
    } else if method == "get" && is_workflow_plan_revision_item_path(path) {
        Some("WorkflowPlanRevisionSuccess200".into())
    } else if method == "get" && is_workflow_run_start_path(path) {
        Some("WorkflowRunListSuccess200".into())
    } else if method == "get"
        && (is_workflow_run_item_path(path) || is_workflow_run_wait_path(path))
    {
        Some("WorkflowRunSuccess200".into())
    } else if method == "get" && is_workflow_run_output_path(path) {
        Some("WorkflowRunOutputSuccess200".into())
    } else if method == "get" && is_workflow_run_variables_path(path) {
        Some("WorkflowRunVariableInspectionSuccess200".into())
    } else if method == "get" && is_workflow_run_diagnostics_path(path) {
        Some("WorkflowRunDiagnosticsSuccess200".into())
    } else if method == "get" && is_workflow_run_history_path(path) {
        Some("WorkflowRunHistoryPageSuccess200".into())
    } else {
        None
    }
}

fn is_ontology_collection_path(path: &str) -> bool {
    path.contains("/projects/{project_id}/") && path.ends_with("/ontologies")
}

fn is_ontology_item_path(path: &str) -> bool {
    path.ends_with("/ontologies/{ontology_id}")
}

fn is_ontology_revision_item_path(path: &str) -> bool {
    path.ends_with("/ontologies/{ontology_id}/revisions/{revision_id}")
}

fn is_ontology_diff_path(path: &str) -> bool {
    path.ends_with("/ontologies/{ontology_id}/revisions/{from_revision_id}/diff/{to_revision_id}")
}

fn is_human_task_collection_path(path: &str) -> bool {
    path.contains("/projects/{project_id}/") && path.ends_with("/human-tasks")
}

fn is_human_task_item_path(path: &str) -> bool {
    path.ends_with("/human-tasks/{human_task_id}")
}

fn is_human_task_mutation_path(path: &str) -> bool {
    is_human_task_assignment_mutation_path(path) || is_human_task_submission_path(path)
}

fn is_workflow_goal_mutation_path(path: &str) -> bool {
    path.ends_with("/workflow-goals")
}

fn is_workflow_node_catalog_path(path: &str) -> bool {
    path.contains("/projects/{project_id}/") && path.ends_with("/workflow-node-catalog")
}

fn is_workflow_goal_item_path(path: &str) -> bool {
    path.ends_with("/workflow-goals/{workflow_goal_id}")
}

fn is_workflow_plan_revision_item_path(path: &str) -> bool {
    path.contains("/workflow-goals/{workflow_goal_id}/")
        && path.ends_with("/plan-revisions/{plan_revision_id}")
}

fn is_workflow_run_item_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}")
}

fn is_workflow_run_wait_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}/wait")
}

fn is_workflow_run_output_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}/output")
}

fn is_workflow_run_variables_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}/variables")
}

fn is_workflow_run_diagnostics_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}/diagnostics")
}

fn is_workflow_run_history_path(path: &str) -> bool {
    path.ends_with("/workflow-runs/{workflow_run_id}/history")
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
