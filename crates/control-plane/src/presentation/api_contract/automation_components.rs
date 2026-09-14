use serde_json::{Map, Value, json};

pub(super) const AUTOMATION_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    (
        "AutomationWebhookEndpointSuccessResponse",
        "AutomationWebhookEndpoint",
    ),
    (
        "AutomationDefinitionSuccessResponse",
        "AutomationDefinition",
    ),
    (
        "AutomationDefinitionListSuccessResponse",
        "AutomationDefinitionList",
    ),
    ("AutomationRevisionSuccessResponse", "AutomationRevision"),
];

pub(super) const AUTOMATION_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "AutomationWebhookEndpointSuccess200",
        200,
        "AutomationWebhookEndpointSuccessResponse",
    ),
    (
        "AutomationWebhookEndpointSuccess201",
        201,
        "AutomationWebhookEndpointSuccessResponse",
    ),
    (
        "AutomationDefinitionSuccess200",
        200,
        "AutomationDefinitionSuccessResponse",
    ),
    (
        "AutomationDefinitionListSuccess200",
        200,
        "AutomationDefinitionListSuccessResponse",
    ),
    (
        "AutomationRevisionSuccess200",
        200,
        "AutomationRevisionSuccessResponse",
    ),
];

pub(super) fn install_automation_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("AutomationWebhookEndpoint", endpoint_schema()),
        ("AutomationDefinition", definition_schema()),
        (
            "AutomationDefinitionList",
            json!({
                "type": "array",
                "items": {"$ref": "#/components/schemas/AutomationDefinition"}
            }),
        ),
        ("AutomationRevision", revision_schema()),
    ] {
        schemas.insert(name.into(), schema);
    }
}

fn endpoint_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId",
            "projectId",
            "environmentId",
            "endpointId",
            "endpointKey",
            "automationId",
            "revisionId",
            "revisionDigest",
            "signatureAlgorithm",
            "signingSecret",
            "requestSchemaDigest",
            "maxBodyBytes",
            "generation",
            "state",
            "createdAt"
        ],
        "properties": {
            "organizationId": {"type": "string", "format": "uuid"},
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "endpointId": {"type": "string", "format": "uuid"},
            "endpointKey": {"type": "string", "minLength": 1},
            "automationId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"},
            "revisionDigest": {"type": "string", "minLength": 1},
            "signatureAlgorithm": {"type": "string", "enum": ["hmac_sha256"]},
            "signingSecret": {
                "type": "object",
                "additionalProperties": false,
                "required": ["secretId", "version"],
                "properties": {
                    "secretId": {"type": "string", "format": "uuid"},
                    "version": {"type": "integer", "minimum": 1}
                }
            },
            "requestSchemaDigest": {"type": "string", "minLength": 1},
            "maxBodyBytes": {"type": "integer", "minimum": 1},
            "generation": {"type": "integer", "minimum": 1},
            "state": {"type": "string", "enum": ["active", "disabled", "revoked"]},
            "createdAt": {"type": "string", "format": "date-time"},
            "stateChangedAt": {"type": ["string", "null"], "format": "date-time"}
        }
    })
}

fn definition_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId",
            "projectId",
            "environmentId",
            "automationId",
            "name",
            "triggerKind",
            "revisionId",
            "revisionNumber",
            "revisionDigest",
            "definitionAcl",
            "createdAt",
            "updatedAt"
        ],
        "properties": {
            "organizationId": {"type": "string", "format": "uuid"},
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "automationId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1},
            "triggerKind": {
                "type": "string",
                "enum": ["schedule", "webhook", "plugin_event", "source_event"]
            },
            "revisionId": {"type": "string", "format": "uuid"},
            "revisionNumber": {"type": "integer", "minimum": 1},
            "revisionDigest": {"type": "string", "minLength": 1},
            "definitionAcl": {"type": "string", "minLength": 1},
            "createdAt": {"type": "string", "format": "date-time"},
            "updatedAt": {"type": "string", "format": "date-time"}
        }
    })
}

fn revision_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId",
            "projectId",
            "environmentId",
            "automationId",
            "revisionId",
            "revisionNumber",
            "revisionAcl",
            "revisionDigest"
        ],
        "properties": {
            "organizationId": {"type": "string", "format": "uuid"},
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "automationId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"},
            "revisionNumber": {"type": "integer", "minimum": 1},
            "parentRevisionId": {"type": ["string", "null"], "format": "uuid"},
            "parentDigest": {"type": ["string", "null"]},
            "revisionAcl": {"type": "string", "minLength": 1},
            "revisionDigest": {"type": "string", "minLength": 1}
        }
    })
}
