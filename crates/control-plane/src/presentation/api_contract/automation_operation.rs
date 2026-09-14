use a3s_cloud_contracts::AUTOMATION_WEBHOOK_MAX_BODY_BYTES;
use crate::modules::automations::{
    DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT, MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
};
use serde_json::{json, Value};

pub(super) fn is_automation_management_path(path: &str) -> bool {
    path.contains("automation-webhook-endpoints") || path.contains("automation-definitions")
}

pub(super) fn is_automation_webhook_endpoint_collection_path(path: &str) -> bool {
    path.ends_with("/automation-webhook-endpoints")
}

pub(super) fn is_automation_webhook_endpoint_item_path(path: &str) -> bool {
    path.contains("/automation-webhook-endpoints/{endpoint_id}")
        && !path.ends_with("/disable")
        && !path.ends_with("/enable")
        && !path.ends_with("/revoke")
}

pub(super) fn is_automation_webhook_endpoint_lifecycle_path(path: &str) -> bool {
    path.ends_with("/automation-webhook-endpoints/{endpoint_id}/disable")
        || path.ends_with("/automation-webhook-endpoints/{endpoint_id}/enable")
        || path.ends_with("/automation-webhook-endpoints/{endpoint_id}/revoke")
}

pub(super) fn is_automation_webhook_endpoint_mutation_path(path: &str) -> bool {
    is_automation_webhook_endpoint_collection_path(path)
        || is_automation_webhook_endpoint_lifecycle_path(path)
}

pub(super) fn is_automation_definition_collection_path(path: &str) -> bool {
    path.ends_with("/automation-definitions")
}

pub(super) fn is_automation_definition_item_path(path: &str) -> bool {
    path.contains("/automation-definitions/{automation_id}") && !path.contains("/revisions/")
}

pub(super) fn is_automation_revision_item_path(path: &str) -> bool {
    path.ends_with("/automation-definitions/{automation_id}/revisions/{revision_id}")
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method != "get" || !is_automation_definition_collection_path(path) {
        return Vec::new();
    }
    vec![json!({
        "name": "limit",
        "in": "query",
        "required": false,
        "schema": {
            "type": "integer",
            "minimum": 1,
            "maximum": MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
            "default": DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT
        },
        "description": "Maximum number of Automation definitions to return."
    })]
}

pub(super) fn request_schema(path: &str) -> Option<Value> {
    if is_automation_webhook_endpoint_collection_path(path) {
        return Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "endpointId",
                "endpointKey",
                "signingSecret",
                "maxBodyBytes",
                "automationId",
                "revisionId"
            ],
            "properties": {
                "endpointId": {"type": "string", "format": "uuid"},
                "endpointKey": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 128,
                    "pattern": "^[A-Za-z0-9._~-]+$"
                },
                "signingSecret": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["secretId", "version"],
                    "properties": {
                        "secretId": {"type": "string", "format": "uuid"},
                        "version": {"type": "integer", "minimum": 1}
                    }
                },
                "maxBodyBytes": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": AUTOMATION_WEBHOOK_MAX_BODY_BYTES
                },
                "automationId": {"type": "string", "format": "uuid"},
                "revisionId": {"type": "string", "format": "uuid"}
            }
        }));
    }
    if is_automation_webhook_endpoint_lifecycle_path(path) {
        return Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["expectedGeneration"],
            "properties": {
                "expectedGeneration": {"type": "integer", "minimum": 1}
            }
        }));
    }
    None
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<&'static str> {
    match (method, status) {
        ("post", 201) if is_automation_webhook_endpoint_collection_path(path) => {
            Some("AutomationWebhookEndpointSuccess201")
        }
        ("post", 200) if is_automation_webhook_endpoint_lifecycle_path(path) => {
            Some("AutomationWebhookEndpointSuccess200")
        }
        ("get", 200) if is_automation_webhook_endpoint_item_path(path) => {
            Some("AutomationWebhookEndpointSuccess200")
        }
        ("get", 200) if is_automation_definition_collection_path(path) => {
            Some("AutomationDefinitionListSuccess200")
        }
        ("get", 200) if is_automation_definition_item_path(path) => {
            Some("AutomationDefinitionSuccess200")
        }
        ("get", 200) if is_automation_revision_item_path(path) => {
            Some("AutomationRevisionSuccess200")
        }
        _ => None,
    }
}
