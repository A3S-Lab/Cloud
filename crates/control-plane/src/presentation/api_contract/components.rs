use super::OPENAPI_CONTRACT_VERSION;
use crate::modules::notifications::{
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
    MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS, NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES,
    NOTIFICATION_ALERT_POLICY_SCHEMA, OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4,
};
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
    let outbound_subscription_success =
        typed_success_response_schema("#/components/schemas/OutboundNotificationSubscription");
    let outbound_subscription_page_success =
        typed_success_response_schema("#/components/schemas/OutboundNotificationSubscriptionPage");
    let outbound_subscription_mutation_success = typed_success_response_schema(
        "#/components/schemas/OutboundNotificationSubscriptionMutation",
    );
    let alert_policy_success =
        typed_success_response_schema("#/components/schemas/NotificationAlertPolicy");
    let alert_policy_page_success =
        typed_success_response_schema("#/components/schemas/NotificationAlertPolicyPage");
    let alert_policy_mutation_success =
        typed_success_response_schema("#/components/schemas/NotificationAlertPolicyMutation");
    let recipient_contact_success =
        typed_success_response_schema("#/components/schemas/RecipientContact");
    let recipient_contact_list_success =
        typed_success_response_schema("#/components/schemas/RecipientContactList");
    let recipient_contact_mutation_success =
        typed_success_response_schema("#/components/schemas/RecipientContactMutation");
    let outbound_notification_subscription = outbound_notification_subscription_schema();
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
            },
            "RecipientContact": recipient_contact_schema(false),
            "RecipientContactList": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/RecipientContact" }
            },
            "RecipientContactMutation": recipient_contact_schema(true),
            "RecipientContactSuccessResponse": recipient_contact_success,
            "RecipientContactListSuccessResponse": recipient_contact_list_success,
            "RecipientContactMutationSuccessResponse": recipient_contact_mutation_success,
            "NotificationAlertPolicy": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "organizationId", "policyId", "source",
                    "projectId", "environmentId", "notifyOnRecovery", "definitionSchema",
                    "definitionAcl", "definitionDigest", "state", "aggregateVersion",
                    "createdBy", "createdAt", "revokedAt"
                ],
                "properties": {
                    "organizationId": { "type": "string", "format": "uuid" },
                    "policyId": { "type": "string", "format": "uuid" },
                    "source": {
                        "type": "string",
                        "enum": [
                            "edge.domain-claim-status.v1",
                            "edge.gateway-certificate-renewal-status.v1",
                            "workload.deployment-health.v1",
                            "edge.gateway-certificate-expiry-status.v1"
                        ]
                    },
                    "projectId": { "type": "string", "format": "uuid" },
                    "environmentId": { "type": "string", "format": "uuid" },
                    "notifyOnRecovery": { "type": "boolean" },
                    "definitionSchema": {
                        "type": "string",
                        "enum": [NOTIFICATION_ALERT_POLICY_SCHEMA]
                    },
                    "definitionAcl": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES
                    },
                    "definitionDigest": {
                        "type": "string",
                        "pattern": "^sha256:[0-9a-f]{64}$"
                    },
                    "state": { "type": "string", "enum": ["active", "revoked"] },
                    "aggregateVersion": { "type": "integer", "minimum": 1, "maximum": 2 },
                    "createdBy": { "type": "string", "format": "uuid" },
                    "createdAt": { "type": "string", "format": "date-time" },
                    "revokedAt": { "type": "string", "format": "date-time", "nullable": true }
                }
            },
            "NotificationAlertPolicyPage": {
                "type": "object",
                "additionalProperties": false,
                "required": ["policies", "nextCursor"],
                "properties": {
                    "policies": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/NotificationAlertPolicy" }
                    },
                    "nextCursor": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 128,
                        "nullable": true
                    }
                }
            },
            "NotificationAlertPolicyMutation": {
                "type": "object",
                "additionalProperties": false,
                "required": ["policy", "replayed"],
                "properties": {
                    "policy": { "$ref": "#/components/schemas/NotificationAlertPolicy" },
                    "replayed": { "type": "boolean" }
                }
            },
            "NotificationAlertPolicySuccessResponse": alert_policy_success,
            "NotificationAlertPolicyPageSuccessResponse": alert_policy_page_success,
            "NotificationAlertPolicyMutationSuccessResponse": alert_policy_mutation_success,
            "OutboundNotificationConnectorTarget": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "projectId", "environmentId", "profileId", "revisionId"],
                "properties": {
                    "kind": { "type": "string", "enum": ["connector"] },
                    "projectId": { "type": "string", "format": "uuid" },
                    "environmentId": { "type": "string", "format": "uuid" },
                    "profileId": { "type": "string", "format": "uuid" },
                    "revisionId": { "type": "string", "format": "uuid" }
                }
            },
            "OutboundNotificationRecipientContactTarget": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "recipientContactId"],
                "properties": {
                    "kind": { "type": "string", "enum": ["recipient_contact"] },
                    "recipientContactId": { "type": "string", "format": "uuid" }
                }
            },
            "OutboundNotificationTarget": {
                "oneOf": [
                    { "$ref": "#/components/schemas/OutboundNotificationConnectorTarget" },
                    { "$ref": "#/components/schemas/OutboundNotificationRecipientContactTarget" }
                ],
                "discriminator": {
                    "propertyName": "kind",
                    "mapping": {
                        "connector": "#/components/schemas/OutboundNotificationConnectorTarget",
                        "recipient_contact": "#/components/schemas/OutboundNotificationRecipientContactTarget"
                    }
                }
            },
            "OutboundNotificationSubscription": outbound_notification_subscription,
            "OutboundNotificationSubscriptionPage": {
                "type": "object",
                "additionalProperties": false,
                "required": ["subscriptions", "nextCursor"],
                "properties": {
                    "subscriptions": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/OutboundNotificationSubscription" }
                    },
                    "nextCursor": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 128,
                        "nullable": true
                    }
                }
            },
            "OutboundNotificationSubscriptionMutation": {
                "type": "object",
                "additionalProperties": false,
                "required": ["subscription", "replayed"],
                "properties": {
                    "subscription": {
                        "$ref": "#/components/schemas/OutboundNotificationSubscription"
                    },
                    "replayed": { "type": "boolean" }
                }
            },
            "OutboundNotificationSubscriptionSuccessResponse": outbound_subscription_success,
            "OutboundNotificationSubscriptionPageSuccessResponse": outbound_subscription_page_success,
            "OutboundNotificationSubscriptionMutationSuccessResponse": outbound_subscription_mutation_success
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
        "RecipientContactSuccess200".into(),
        response_component(200, "#/components/schemas/RecipientContactSuccessResponse"),
    );
    response_components.insert(
        "RecipientContactListSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/RecipientContactListSuccessResponse",
        ),
    );
    for status in [200, 202] {
        response_components.insert(
            format!("RecipientContactMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/RecipientContactMutationSuccessResponse",
            ),
        );
    }
    response_components.insert(
        "NotificationAlertPolicySuccess200".into(),
        response_component(
            200,
            "#/components/schemas/NotificationAlertPolicySuccessResponse",
        ),
    );
    response_components.insert(
        "NotificationAlertPolicyPageSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/NotificationAlertPolicyPageSuccessResponse",
        ),
    );
    for status in [200, 201] {
        response_components.insert(
            format!("NotificationAlertPolicyMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/NotificationAlertPolicyMutationSuccessResponse",
            ),
        );
    }
    response_components.insert(
        "OutboundNotificationSubscriptionSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/OutboundNotificationSubscriptionSuccessResponse",
        ),
    );
    response_components.insert(
        "OutboundNotificationSubscriptionPageSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/OutboundNotificationSubscriptionPageSuccessResponse",
        ),
    );
    for status in [200, 201] {
        response_components.insert(
            format!("OutboundNotificationSubscriptionMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/OutboundNotificationSubscriptionMutationSuccessResponse",
            ),
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

fn outbound_notification_subscription_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "subscriptionId", "channel", "minimumSeverity",
            "target", "connectorProjectId", "connectorEnvironmentId",
            "connectorProfileId", "connectorRevisionId", "maximumProviderAttempts",
            "suppressBefore", "definitionSchema", "definitionAcl", "definitionDigest",
            "state", "aggregateVersion", "createdBy", "createdAt", "revokedAt"
        ],
        "properties": {
            "organizationId": { "type": "string", "format": "uuid" },
            "subscriptionId": { "type": "string", "format": "uuid" },
            "channel": {
                "type": "string",
                "enum": ["signed_webhook", "slack_compatible", "smtp"]
            },
            "minimumSeverity": {
                "type": "string",
                "enum": ["information", "warning", "critical"]
            },
            "target": { "$ref": "#/components/schemas/OutboundNotificationTarget" },
            "connectorProjectId": legacy_connector_target_projection_schema(),
            "connectorEnvironmentId": legacy_connector_target_projection_schema(),
            "connectorProfileId": legacy_connector_target_projection_schema(),
            "connectorRevisionId": legacy_connector_target_projection_schema(),
            "maximumProviderAttempts": {
                "type": "integer",
                "minimum": MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
                "maximum": MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
            },
            "suppressBefore": {
                "type": "string",
                "format": "date-time",
                "nullable": true
            },
            "definitionSchema": {
                "type": "string",
                "enum": [
                    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
                    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2,
                    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3,
                    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4
                ]
            },
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES
            },
            "definitionDigest": {
                "type": "string",
                "pattern": "^sha256:[0-9a-f]{64}$"
            },
            "state": { "type": "string", "enum": ["active", "revoked"] },
            "aggregateVersion": { "type": "integer", "minimum": 1, "maximum": 2 },
            "createdBy": { "type": "string", "format": "uuid" },
            "createdAt": { "type": "string", "format": "date-time" },
            "revokedAt": { "type": "string", "format": "date-time", "nullable": true }
        }
    })
}

fn legacy_connector_target_projection_schema() -> Value {
    json!({
        "type": "string",
        "format": "uuid",
        "nullable": true,
        "deprecated": true,
        "description": "Deprecated non-authoritative compatibility projection. Use target; null for SMTP."
    })
}

fn typed_success_response_schema(data_schema_ref: &str) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["code", "message", "data", "requestId", "timestamp"],
        "properties": {
            "code": { "type": "integer", "minimum": 200, "maximum": 399 },
            "message": { "type": "string" },
            "data": {},
            "requestId": { "type": "string", "format": "uuid" },
            "timestamp": { "type": "string", "format": "date-time" }
        },
        "allOf": [
            {
                "type": "object",
                "required": ["data"],
                "properties": {
                    "data": { "$ref": data_schema_ref }
                }
            }
        ]
    })
}

fn recipient_contact_schema(include_replayed: bool) -> Value {
    let mut required = vec![
        "id",
        "principalId",
        "addressDigest",
        "addressHint",
        "aggregateVersion",
        "status",
        "createdAt",
        "updatedAt",
        "verifiedAt",
        "revokedAt",
    ];
    let mut properties = json!({
        "id": { "type": "string", "format": "uuid" },
        "principalId": { "type": "string", "format": "uuid" },
        "addressDigest": {
            "type": "string",
            "pattern": "^sha256:[0-9a-f]{64}$"
        },
        "addressHint": {
            "type": "string",
            "minLength": 5,
            "maxLength": 257,
            "pattern": "^\\*\\*\\*@[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"
        },
        "aggregateVersion": { "type": "integer", "minimum": 1 },
        "status": { "type": "string", "enum": ["pending", "verified", "revoked"] },
        "createdAt": { "type": "string", "format": "date-time" },
        "updatedAt": { "type": "string", "format": "date-time" },
        "verifiedAt": { "type": "string", "format": "date-time", "nullable": true },
        "revokedAt": { "type": "string", "format": "date-time", "nullable": true }
    });
    if include_replayed {
        required.push("replayed");
        properties["replayed"] = json!({ "type": "boolean" });
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
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
