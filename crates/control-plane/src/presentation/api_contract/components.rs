use super::agent_components::install_agent_component_schemas;
use super::developer_workflow_components::{
    install_developer_workflow_component_schemas, BUILD_PLAN_SUCCESS_RESPONSE_BINDINGS,
    BUILD_PLAN_SUCCESS_SCHEMA_BINDINGS,
};
use super::preview_management_components::{
    install_preview_management_component_schemas, PREVIEW_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS,
    PREVIEW_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS,
};
use super::workflow_components::install_workflow_component_schemas;
use super::workflow_goal_components::install_workflow_goal_component_schemas;
use super::workflow_human_task_components::install_workflow_human_task_component_schemas;
use super::workflow_ontology_components::install_workflow_ontology_component_schemas;
use super::workflow_run_components::install_workflow_run_component_schemas;
use super::workflow_run_observation_components::install_workflow_run_observation_component_schemas;
use super::workload_profile_components::{
    install_workload_profile_component_schemas, WORKLOAD_PROFILE_SUCCESS_RESPONSE_BINDINGS,
    WORKLOAD_PROFILE_SUCCESS_SCHEMA_BINDINGS,
};
use super::OPENAPI_CONTRACT_VERSION;
use crate::modules::connectors::{
    CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES,
    CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES, CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE, MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
};
use crate::modules::notifications::{
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
    MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS, NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES,
    NOTIFICATION_ALERT_POLICY_SCHEMA, NOTIFICATION_ALERT_POLICY_SCHEMA_V2,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4,
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
    let security_gateway_route_policy_timeline_page_success = typed_success_response_schema(
        "#/components/schemas/SecurityGatewayRoutePolicyTimelinePage",
    );
    let workflow_definition_success =
        typed_success_response_schema("#/components/schemas/WorkflowDefinition");
    let workflow_definition_list_success =
        typed_success_response_schema("#/components/schemas/WorkflowDefinitionList");
    let workflow_revision_summary_list_success =
        typed_success_response_schema("#/components/schemas/WorkflowRevisionSummaryList");
    let workflow_revision_success =
        typed_success_response_schema("#/components/schemas/WorkflowRevision");
    let workflow_definition_mutation_success =
        typed_success_response_schema("#/components/schemas/WorkflowDefinitionMutation");
    let outbound_notification_subscription = outbound_notification_subscription_schema();
    let mut schema_components = json!({
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
            "SecurityGatewayRoutePolicyTimelineEntry": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "eventId", "eventKey", "schemaVersion", "organizationId", "projectId",
                    "environmentId", "routeId", "policyRevision", "policyDigest",
                    "occurredAt", "correlationId", "auditCorrelation", "auditRecordId",
                    "actorPrincipalId"
                ],
                "properties": {
                    "eventId": { "type": "string", "format": "uuid" },
                    "eventKey": {
                        "type": "string",
                        "enum": [
                            "edge.mcp-route-policy.created",
                            "edge.mcp-route-policy.revised"
                        ]
                    },
                    "schemaVersion": { "type": "integer", "enum": [1] },
                    "organizationId": { "type": "string", "format": "uuid" },
                    "projectId": { "type": "string", "format": "uuid" },
                    "environmentId": { "type": "string", "format": "uuid" },
                    "routeId": { "type": "string", "format": "uuid" },
                    "policyRevision": {
                        "type": "integer", "minimum": 1, "maximum": 9007199254740991_i64
                    },
                    "policyDigest": {
                        "type": "string", "pattern": "^sha256:[0-9a-f]{64}$"
                    },
                    "occurredAt": { "type": "string", "format": "date-time" },
                    "correlationId": { "type": "string", "format": "uuid" },
                    "auditCorrelation": { "type": "string", "enum": ["verified", "missing"] },
                    "auditRecordId": {
                        "type": "string", "format": "uuid", "nullable": true
                    },
                    "actorPrincipalId": {
                        "type": "string", "format": "uuid", "nullable": true
                    }
                }
            },
            "SecurityGatewayRoutePolicyTimelinePage": {
                "type": "object",
                "additionalProperties": false,
                "required": ["entries", "nextCursor"],
                "properties": {
                    "entries": {
                        "type": "array",
                        "items": {
                            "$ref": "#/components/schemas/SecurityGatewayRoutePolicyTimelineEntry"
                        }
                    },
                    "nextCursor": {
                        "type": "string", "minLength": 1, "maxLength": 128, "nullable": true
                    }
                }
            },
            "SecurityGatewayRoutePolicyTimelinePageSuccessResponse":
                security_gateway_route_policy_timeline_page_success,
            "RecipientContact": recipient_contact_schema(false),
            "RecipientContactList": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/RecipientContact" }
            },
            "RecipientContactMutation": recipient_contact_schema(true),
            "RecipientContactSuccessResponse": recipient_contact_success,
            "RecipientContactListSuccessResponse": recipient_contact_list_success,
            "RecipientContactMutationSuccessResponse": recipient_contact_mutation_success,
            "NotificationAlertPolicyEnvironmentTarget": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "projectId", "environmentId"],
                "properties": {
                    "kind": { "type": "string", "enum": ["environment"] },
                    "projectId": { "type": "string", "format": "uuid" },
                    "environmentId": { "type": "string", "format": "uuid" }
                }
            },
            "NotificationAlertPolicyNodeTarget": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "nodeId"],
                "properties": {
                    "kind": { "type": "string", "enum": ["node"] },
                    "nodeId": { "type": "string", "format": "uuid" }
                }
            },
            "NotificationAlertPolicyTarget": {
                "oneOf": [
                    { "$ref": "#/components/schemas/NotificationAlertPolicyEnvironmentTarget" },
                    { "$ref": "#/components/schemas/NotificationAlertPolicyNodeTarget" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "NotificationAlertPolicy": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "organizationId", "policyId", "source", "target",
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
                            "edge.gateway-certificate-expiry-status.v1",
                            "fleet.node-availability-status.v1"
                        ]
                    },
                    "target": { "$ref": "#/components/schemas/NotificationAlertPolicyTarget" },
                    "projectId": { "type": "string", "format": "uuid", "nullable": true },
                    "environmentId": { "type": "string", "format": "uuid", "nullable": true },
                    "notifyOnRecovery": { "type": "boolean" },
                    "definitionSchema": {
                        "type": "string",
                        "enum": [
                            NOTIFICATION_ALERT_POLICY_SCHEMA,
                            NOTIFICATION_ALERT_POLICY_SCHEMA_V2
                        ]
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
        })
        .as_object()
        .cloned()
        .ok_or_else(|| BootError::Internal("generated OpenAPI schemas are invalid".into()))?;
    install_connector_component_schemas(&mut schema_components)?;
    install_agent_component_schemas(&mut schema_components);
    install_developer_workflow_component_schemas(&mut schema_components);
    install_workload_profile_component_schemas(&mut schema_components);
    install_preview_management_component_schemas(&mut schema_components);
    install_workflow_component_schemas(&mut schema_components);
    install_workflow_goal_component_schemas(&mut schema_components);
    install_workflow_human_task_component_schemas(&mut schema_components);
    install_workflow_ontology_component_schemas(&mut schema_components);
    install_workflow_run_component_schemas(&mut schema_components);
    install_workflow_run_observation_component_schemas(&mut schema_components);
    for &(name, data_schema) in BUILD_PLAN_SUCCESS_SCHEMA_BINDINGS {
        schema_components.insert(
            name.into(),
            typed_success_response_schema(&format!("#/components/schemas/{data_schema}")),
        );
    }
    for &(name, data_schema) in WORKLOAD_PROFILE_SUCCESS_SCHEMA_BINDINGS {
        schema_components.insert(
            name.into(),
            typed_success_response_schema(&format!("#/components/schemas/{data_schema}")),
        );
    }
    for &(name, data_schema) in PREVIEW_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS {
        schema_components.insert(
            name.into(),
            typed_success_response_schema(&format!("#/components/schemas/{data_schema}")),
        );
    }
    schema_components.insert(
        "WorkflowDefinitionSuccessResponse".into(),
        workflow_definition_success,
    );
    schema_components.insert(
        "WorkflowDefinitionListSuccessResponse".into(),
        workflow_definition_list_success,
    );
    schema_components.insert(
        "WorkflowRevisionSummaryListSuccessResponse".into(),
        workflow_revision_summary_list_success,
    );
    schema_components.insert(
        "WorkflowRevisionSuccessResponse".into(),
        workflow_revision_success,
    );
    schema_components.insert(
        "WorkflowDefinitionMutationSuccessResponse".into(),
        workflow_definition_mutation_success,
    );
    for (name, data_schema) in [
        ("WorkflowNodeCatalogSuccessResponse", "WorkflowNodeCatalog"),
        ("WorkflowGoalSuccessResponse", "WorkflowGoal"),
        ("WorkflowGoalListSuccessResponse", "WorkflowGoalList"),
        (
            "WorkflowGoalMutationSuccessResponse",
            "WorkflowGoalMutation",
        ),
        (
            "WorkflowPlanRevisionSuccessResponse",
            "WorkflowPlanRevision",
        ),
        ("WorkflowRunSuccessResponse", "WorkflowRun"),
        ("WorkflowRunListSuccessResponse", "WorkflowRunList"),
        ("WorkflowRunMutationSuccessResponse", "WorkflowRunMutation"),
        ("WorkflowRunOutputSuccessResponse", "WorkflowRunOutput"),
        (
            "WorkflowRunVariableInspectionSuccessResponse",
            "WorkflowRunVariableInspection",
        ),
        (
            "WorkflowRunDiagnosticsSuccessResponse",
            "WorkflowRunDiagnostics",
        ),
        (
            "WorkflowRunHistoryPageSuccessResponse",
            "WorkflowRunHistoryPage",
        ),
        ("OntologySuccessResponse", "Ontology"),
        ("OntologyListSuccessResponse", "OntologyList"),
        (
            "OntologyRevisionSummaryListSuccessResponse",
            "OntologyRevisionSummaryList",
        ),
        ("OntologyRevisionSuccessResponse", "OntologyRevision"),
        ("OntologyMutationSuccessResponse", "OntologyMutation"),
        ("OntologyDiffSuccessResponse", "OntologyRevisionDiff"),
        ("HumanTaskSuccessResponse", "HumanTask"),
        ("HumanTaskListSuccessResponse", "HumanTaskList"),
        ("HumanTaskMutationSuccessResponse", "HumanTaskMutation"),
        ("AgentConversationSuccessResponse", "AgentConversation"),
        (
            "AgentConversationListSuccessResponse",
            "AgentConversationList",
        ),
        (
            "AgentConversationMutationSuccessResponse",
            "AgentConversationMutation",
        ),
        ("AgentExecutionSuccessResponse", "AgentExecution"),
        ("AgentExecutionListSuccessResponse", "AgentExecutionList"),
        (
            "AgentExecutionMutationSuccessResponse",
            "AgentExecutionMutation",
        ),
        (
            "AgentExecutionChangeSetSuccessResponse",
            "AgentExecutionChangeSet",
        ),
        (
            "AgentExecutionEventPageSuccessResponse",
            "AgentExecutionEventPage",
        ),
        (
            "AgentExecutionCheckpointSuccessResponse",
            "AgentExecutionCheckpoint",
        ),
        (
            "AgentExecutionCheckpointListSuccessResponse",
            "AgentExecutionCheckpointList",
        ),
        (
            "AgentExecutionCheckpointMutationSuccessResponse",
            "AgentExecutionCheckpointMutation",
        ),
        (
            "AgentExecutionCheckpointSnapshotSuccessResponse",
            "AgentExecutionCheckpointSnapshot",
        ),
        (
            "AgentExecutionTrajectoryPageSuccessResponse",
            "AgentExecutionTrajectoryPage",
        ),
        (
            "AgentApprovalCheckpointSuccessResponse",
            "AgentApprovalCheckpoint",
        ),
        (
            "AgentApprovalCheckpointListSuccessResponse",
            "AgentApprovalCheckpointList",
        ),
        (
            "AgentApprovalCheckpointMutationSuccessResponse",
            "AgentApprovalCheckpointMutation",
        ),
    ] {
        schema_components.insert(
            name.into(),
            typed_success_response_schema(&format!("#/components/schemas/{data_schema}")),
        );
    }
    components.insert("schemas".into(), Value::Object(schema_components));

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
    for &(name, status, schema) in BUILD_PLAN_SUCCESS_RESPONSE_BINDINGS {
        response_components.insert(
            name.into(),
            response_component(status, &format!("#/components/schemas/{schema}")),
        );
    }
    for &(name, status, schema) in WORKLOAD_PROFILE_SUCCESS_RESPONSE_BINDINGS {
        response_components.insert(
            name.into(),
            response_component(status, &format!("#/components/schemas/{schema}")),
        );
    }
    for &(name, status, schema) in PREVIEW_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS {
        response_components.insert(
            name.into(),
            response_component(status, &format!("#/components/schemas/{schema}")),
        );
    }
    response_components.insert(
        "SecurityGatewayRoutePolicyTimelinePageSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/SecurityGatewayRoutePolicyTimelinePageSuccessResponse",
        ),
    );
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
    response_components.insert(
        "ConnectorProfileListSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorProfileListSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorProfileRecordSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorProfileRecordSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorRevisionListSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorRevisionListSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorRevisionSuccess200".into(),
        response_component(200, "#/components/schemas/ConnectorRevisionSuccessResponse"),
    );
    response_components.insert(
        "ConnectorRevisionRevocationSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorRevisionRevocationSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorExecutionAttemptPageSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorExecutionAttemptPageSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorExecutionAttemptSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorExecutionAttemptSuccessResponse",
        ),
    );
    response_components.insert(
        "ConnectorExecutionAttemptResolutionSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/ConnectorExecutionAttemptResolutionSuccessResponse",
        ),
    );
    for status in [200, 201] {
        response_components.insert(
            format!("ConnectorProfileMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/ConnectorProfileMutationSuccessResponse",
            ),
        );
        response_components.insert(
            format!("ConnectorRevisionRevocationMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/ConnectorRevisionRevocationMutationSuccessResponse",
            ),
        );
        response_components.insert(
            format!("ConnectorExecutionAttemptResolutionMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/ConnectorExecutionAttemptResolutionMutationSuccessResponse",
            ),
        );
    }
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
        "WorkflowDefinitionSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/WorkflowDefinitionSuccessResponse",
        ),
    );
    response_components.insert(
        "WorkflowDefinitionListSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/WorkflowDefinitionListSuccessResponse",
        ),
    );
    response_components.insert(
        "WorkflowRevisionSummaryListSuccess200".into(),
        response_component(
            200,
            "#/components/schemas/WorkflowRevisionSummaryListSuccessResponse",
        ),
    );
    response_components.insert(
        "WorkflowRevisionSuccess200".into(),
        response_component(200, "#/components/schemas/WorkflowRevisionSuccessResponse"),
    );
    for status in [200, 201] {
        response_components.insert(
            format!("WorkflowDefinitionMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/WorkflowDefinitionMutationSuccessResponse",
            ),
        );
    }
    for (name, schema) in [
        (
            "WorkflowNodeCatalogSuccess200",
            "WorkflowNodeCatalogSuccessResponse",
        ),
        ("WorkflowGoalSuccess200", "WorkflowGoalSuccessResponse"),
        (
            "WorkflowGoalListSuccess200",
            "WorkflowGoalListSuccessResponse",
        ),
        (
            "WorkflowPlanRevisionSuccess200",
            "WorkflowPlanRevisionSuccessResponse",
        ),
        ("WorkflowRunSuccess200", "WorkflowRunSuccessResponse"),
        (
            "WorkflowRunListSuccess200",
            "WorkflowRunListSuccessResponse",
        ),
        (
            "WorkflowRunOutputSuccess200",
            "WorkflowRunOutputSuccessResponse",
        ),
        (
            "WorkflowRunVariableInspectionSuccess200",
            "WorkflowRunVariableInspectionSuccessResponse",
        ),
        (
            "WorkflowRunDiagnosticsSuccess200",
            "WorkflowRunDiagnosticsSuccessResponse",
        ),
        (
            "WorkflowRunHistoryPageSuccess200",
            "WorkflowRunHistoryPageSuccessResponse",
        ),
        ("OntologySuccess200", "OntologySuccessResponse"),
        ("OntologyListSuccess200", "OntologyListSuccessResponse"),
        (
            "OntologyRevisionSummaryListSuccess200",
            "OntologyRevisionSummaryListSuccessResponse",
        ),
        (
            "OntologyRevisionSuccess200",
            "OntologyRevisionSuccessResponse",
        ),
        ("OntologyDiffSuccess200", "OntologyDiffSuccessResponse"),
        ("HumanTaskSuccess200", "HumanTaskSuccessResponse"),
        ("HumanTaskListSuccess200", "HumanTaskListSuccessResponse"),
        (
            "HumanTaskMutationSuccess200",
            "HumanTaskMutationSuccessResponse",
        ),
        (
            "AgentConversationSuccess200",
            "AgentConversationSuccessResponse",
        ),
        (
            "AgentConversationListSuccess200",
            "AgentConversationListSuccessResponse",
        ),
        ("AgentExecutionSuccess200", "AgentExecutionSuccessResponse"),
        (
            "AgentExecutionListSuccess200",
            "AgentExecutionListSuccessResponse",
        ),
        (
            "AgentExecutionChangeSetSuccess200",
            "AgentExecutionChangeSetSuccessResponse",
        ),
        (
            "AgentExecutionEventPageSuccess200",
            "AgentExecutionEventPageSuccessResponse",
        ),
        (
            "AgentExecutionCheckpointSuccess200",
            "AgentExecutionCheckpointSuccessResponse",
        ),
        (
            "AgentExecutionCheckpointListSuccess200",
            "AgentExecutionCheckpointListSuccessResponse",
        ),
        (
            "AgentExecutionCheckpointSnapshotSuccess200",
            "AgentExecutionCheckpointSnapshotSuccessResponse",
        ),
        (
            "AgentExecutionTrajectoryPageSuccess200",
            "AgentExecutionTrajectoryPageSuccessResponse",
        ),
        (
            "AgentApprovalCheckpointSuccess200",
            "AgentApprovalCheckpointSuccessResponse",
        ),
        (
            "AgentApprovalCheckpointListSuccess200",
            "AgentApprovalCheckpointListSuccessResponse",
        ),
    ] {
        response_components.insert(
            name.into(),
            response_component(200, &format!("#/components/schemas/{schema}")),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("WorkflowGoalMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/WorkflowGoalMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("OntologyMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/OntologyMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 202] {
        response_components.insert(
            format!("WorkflowRunMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/WorkflowRunMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("AgentExecutionCheckpointMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/AgentExecutionCheckpointMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 201] {
        response_components.insert(
            format!("AgentConversationMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/AgentConversationMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 202] {
        response_components.insert(
            format!("AgentExecutionMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/AgentExecutionMutationSuccessResponse",
            ),
        );
    }
    for status in [200, 202] {
        response_components.insert(
            format!("AgentApprovalCheckpointMutationSuccess{status}"),
            response_component(
                status,
                "#/components/schemas/AgentApprovalCheckpointMutationSuccessResponse",
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

fn install_connector_component_schemas(schemas: &mut Map<String, Value>) -> Result<()> {
    let profile_list_success =
        typed_success_response_schema("#/components/schemas/ConnectorProfileList");
    let profile_record_success =
        typed_success_response_schema("#/components/schemas/ConnectorProfileRecord");
    let revision_list_success =
        typed_success_response_schema("#/components/schemas/ConnectorRevisionList");
    let revision_success = typed_success_response_schema("#/components/schemas/ConnectorRevision");
    let profile_mutation_success =
        typed_success_response_schema("#/components/schemas/ConnectorProfileMutation");
    let revocation_success =
        typed_success_response_schema("#/components/schemas/ConnectorRevisionRevocation");
    let revocation_mutation_success =
        typed_success_response_schema("#/components/schemas/ConnectorRevisionRevocationMutation");
    let attempt_page_success =
        typed_success_response_schema("#/components/schemas/ConnectorExecutionAttemptPage");
    let attempt_success =
        typed_success_response_schema("#/components/schemas/ConnectorExecutionAttempt");
    let attempt_resolution_success =
        typed_success_response_schema("#/components/schemas/ConnectorExecutionAttemptResolution");
    let attempt_resolution_mutation_success = typed_success_response_schema(
        "#/components/schemas/ConnectorExecutionAttemptResolutionMutation",
    );
    let mut connector_schemas = json!({
        "ConnectorProfile": {
            "type": "object",
            "additionalProperties": false,
            "required": [
                "organizationId", "projectId", "environmentId", "profileId", "name",
                "currentRevisionId", "currentRevisionNumber", "currentRevisionDigest",
                "aggregateVersion", "createdBy", "createdAt", "updatedAt"
            ],
            "properties": {
                "organizationId": { "type": "string", "format": "uuid" },
                "projectId": { "type": "string", "format": "uuid" },
                "environmentId": { "type": "string", "format": "uuid" },
                "profileId": { "type": "string", "format": "uuid" },
                "name": { "type": "string", "minLength": 1, "maxLength": 63 },
                "currentRevisionId": { "type": "string", "format": "uuid" },
                "currentRevisionNumber": { "type": "integer", "minimum": 1 },
                "currentRevisionDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$"
                },
                "aggregateVersion": { "type": "integer", "minimum": 1 },
                "createdBy": { "type": "string", "format": "uuid" },
                "createdAt": { "type": "string", "format": "date-time" },
                "updatedAt": { "type": "string", "format": "date-time" }
            }
        },
        "ConnectorRevision": {
            "type": "object",
            "additionalProperties": false,
            "required": [
                "organizationId", "projectId", "environmentId", "profileId", "revisionId",
                "revisionNumber", "parentRevisionId", "parentDigest", "definitionKind",
                "definitionSchema", "definitionAcl", "definitionDigest", "createdBy",
                "createdAt"
            ],
            "properties": {
                "organizationId": { "type": "string", "format": "uuid" },
                "projectId": { "type": "string", "format": "uuid" },
                "environmentId": { "type": "string", "format": "uuid" },
                "profileId": { "type": "string", "format": "uuid" },
                "revisionId": { "type": "string", "format": "uuid" },
                "revisionNumber": { "type": "integer", "minimum": 1 },
                "parentRevisionId": {
                    "type": "string", "format": "uuid", "nullable": true
                },
                "parentDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$", "nullable": true
                },
                "definitionKind": { "type": "string", "enum": ["http"] },
                "definitionSchema": {
                    "type": "string", "enum": ["cloud.connector.http.v1"]
                },
                "definitionAcl": {
                    "type": "string", "minLength": 1,
                    "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES,
                    "x-a3s-max-canonical-bytes": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
                },
                "definitionDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$"
                },
                "createdBy": { "type": "string", "format": "uuid" },
                "createdAt": { "type": "string", "format": "date-time" }
            }
        },
        "ConnectorProfileList": {
            "type": "array",
            "maxItems": MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
            "items": { "$ref": "#/components/schemas/ConnectorProfile" }
        },
        "ConnectorRevisionList": {
            "type": "array",
            "maxItems": MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
            "items": { "$ref": "#/components/schemas/ConnectorRevision" }
        },
        "ConnectorProfileRecord": {
            "type": "object",
            "additionalProperties": false,
            "required": ["profile", "revision"],
            "properties": {
                "profile": { "$ref": "#/components/schemas/ConnectorProfile" },
                "revision": { "$ref": "#/components/schemas/ConnectorRevision" }
            }
        },
        "ConnectorProfileMutation": {
            "type": "object",
            "additionalProperties": false,
            "required": ["record", "replayed"],
            "properties": {
                "record": { "$ref": "#/components/schemas/ConnectorProfileRecord" },
                "replayed": { "type": "boolean" }
            }
        },
        "ConnectorRevisionRevocation": {
            "type": "object",
            "additionalProperties": false,
            "required": [
                "organizationId", "projectId", "environmentId", "profileId", "revisionId",
                "revisionNumber", "definitionDigest", "reason", "revokedBy", "revokedAt"
            ],
            "properties": {
                "organizationId": { "type": "string", "format": "uuid" },
                "projectId": { "type": "string", "format": "uuid" },
                "environmentId": { "type": "string", "format": "uuid" },
                "profileId": { "type": "string", "format": "uuid" },
                "revisionId": { "type": "string", "format": "uuid" },
                "revisionNumber": { "type": "integer", "minimum": 1 },
                "definitionDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$"
                },
                "reason": {
                    "type": "string", "minLength": 1,
                    "maxLength": CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES,
                    "pattern": "^[^\\u0000-\\u001F\\u007F-\\u009F]+$",
                    "x-a3s-max-utf8-bytes": CONNECTOR_REVISION_REVOCATION_REASON_MAX_BYTES
                },
                "revokedBy": { "type": "string", "format": "uuid" },
                "revokedAt": { "type": "string", "format": "date-time" }
            }
        },
        "ConnectorRevisionRevocationMutation": {
            "type": "object",
            "additionalProperties": false,
            "required": ["revocation", "replayed"],
            "properties": {
                "revocation": {
                    "$ref": "#/components/schemas/ConnectorRevisionRevocation"
                },
                "replayed": { "type": "boolean" }
            }
        },
        "ConnectorExecutionAttempt": {
            "type": "object",
            "additionalProperties": false,
            "required": [
                "organizationId", "projectId", "environmentId", "profileId", "revisionId",
                "attemptId", "requestDigest", "requestBodyBytes", "state", "recoveryState",
                "reservedAt", "leaseExpiresAt", "dispatchStartedAt", "outcomeDeadlineAt",
                "terminalAt", "createdAt", "observedAt", "evidenceOutcome", "responseStatus",
                "responseDigest", "responseBodyBytes", "retryAfterSeconds", "evidenceStartedAt",
                "evidenceCompletedAt"
            ],
            "properties": {
                "organizationId": { "type": "string", "format": "uuid" },
                "projectId": { "type": "string", "format": "uuid" },
                "environmentId": { "type": "string", "format": "uuid" },
                "profileId": { "type": "string", "format": "uuid" },
                "revisionId": { "type": "string", "format": "uuid" },
                "attemptId": { "type": "string", "format": "uuid" },
                "requestDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$"
                },
                "requestBodyBytes": { "type": "integer", "minimum": 0, "maximum": 1048576 },
                "state": { "type": "string", "enum": ["reserved", "dispatching", "terminal"] },
                "recoveryState": {
                    "type": "string",
                    "enum": ["reserved", "reservation_expired", "in_flight", "indeterminate", "completed"]
                },
                "reservedAt": { "type": "string", "format": "date-time" },
                "leaseExpiresAt": { "type": "string", "format": "date-time" },
                "dispatchStartedAt": { "type": "string", "format": "date-time", "nullable": true },
                "outcomeDeadlineAt": { "type": "string", "format": "date-time", "nullable": true },
                "terminalAt": { "type": "string", "format": "date-time", "nullable": true },
                "createdAt": { "type": "string", "format": "date-time" },
                "observedAt": { "type": "string", "format": "date-time" },
                "evidenceOutcome": {
                    "type": "string",
                    "enum": ["accepted", "retryable", "rejected", "indeterminate"],
                    "nullable": true
                },
                "responseStatus": { "type": "integer", "minimum": 100, "maximum": 599, "nullable": true },
                "responseDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$", "nullable": true
                },
                "responseBodyBytes": { "type": "integer", "minimum": 0, "maximum": 1048576, "nullable": true },
                "retryAfterSeconds": { "type": "integer", "minimum": 0, "maximum": 86400, "nullable": true },
                "evidenceStartedAt": { "type": "string", "format": "date-time", "nullable": true },
                "evidenceCompletedAt": { "type": "string", "format": "date-time", "nullable": true }
            }
        },
        "ConnectorExecutionAttemptPage": {
            "type": "object",
            "additionalProperties": false,
            "required": ["attempts", "nextCursor"],
            "properties": {
                "attempts": {
                    "type": "array",
                    "maxItems": MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
                    "items": { "$ref": "#/components/schemas/ConnectorExecutionAttempt" }
                },
                "nextCursor": { "type": "string", "minLength": 1, "maxLength": 128, "nullable": true }
            }
        },
        "ConnectorExecutionAttemptResolution": {
            "type": "object",
            "additionalProperties": false,
            "required": [
                "organizationId", "projectId", "environmentId", "profileId", "revisionId",
                "attemptId", "requestDigest", "requestBodyBytes", "dispatchStartedAt",
                "outcomeDeadlineAt", "resolution", "reason", "resolvedBy", "resolvedAt"
            ],
            "properties": {
                "organizationId": { "type": "string", "format": "uuid" },
                "projectId": { "type": "string", "format": "uuid" },
                "environmentId": { "type": "string", "format": "uuid" },
                "profileId": { "type": "string", "format": "uuid" },
                "revisionId": { "type": "string", "format": "uuid" },
                "attemptId": { "type": "string", "format": "uuid" },
                "requestDigest": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" },
                "requestBodyBytes": { "type": "integer", "minimum": 0, "maximum": 1048576 },
                "dispatchStartedAt": { "type": "string", "format": "date-time" },
                "outcomeDeadlineAt": { "type": "string", "format": "date-time" },
                "resolution": { "type": "string", "enum": ["indeterminate"] },
                "reason": {
                    "type": "string", "minLength": 1,
                    "maxLength": CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES,
                    "pattern": "^[^\\u0000-\\u001F\\u007F-\\u009F]+$",
                    "x-a3s-max-utf8-bytes": CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES
                },
                "resolvedBy": { "type": "string", "format": "uuid" },
                "resolvedAt": { "type": "string", "format": "date-time" }
            }
        },
        "ConnectorExecutionAttemptResolutionMutation": {
            "type": "object",
            "additionalProperties": false,
            "required": ["resolution", "replayed"],
            "properties": {
                "resolution": { "$ref": "#/components/schemas/ConnectorExecutionAttemptResolution" },
                "replayed": { "type": "boolean" }
            }
        },
        "ConnectorProfileListSuccessResponse": profile_list_success,
        "ConnectorProfileRecordSuccessResponse": profile_record_success,
        "ConnectorRevisionListSuccessResponse": revision_list_success,
        "ConnectorRevisionSuccessResponse": revision_success,
        "ConnectorProfileMutationSuccessResponse": profile_mutation_success,
        "ConnectorRevisionRevocationSuccessResponse": revocation_success,
        "ConnectorRevisionRevocationMutationSuccessResponse": revocation_mutation_success,
        "ConnectorExecutionAttemptPageSuccessResponse": attempt_page_success,
        "ConnectorExecutionAttemptSuccessResponse": attempt_success,
        "ConnectorExecutionAttemptResolutionSuccessResponse": attempt_resolution_success,
        "ConnectorExecutionAttemptResolutionMutationSuccessResponse": attempt_resolution_mutation_success
    })
    .as_object()
    .cloned()
    .ok_or_else(|| BootError::Internal("generated Connector OpenAPI schemas are invalid".into()))?;
    connector_schemas
        .get_mut("ConnectorExecutionAttempt")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            BootError::Internal("generated Connector execution-attempt schema is invalid".into())
        })?
        .insert("example".into(), connector_execution_attempt_example());
    connector_schemas
        .get_mut("ConnectorExecutionAttemptResolution")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            BootError::Internal(
                "generated Connector execution-attempt resolution schema is invalid".into(),
            )
        })?
        .insert(
            "example".into(),
            connector_execution_attempt_resolution_example(),
        );
    schemas.extend(connector_schemas);
    Ok(())
}

fn connector_execution_attempt_example() -> Value {
    json!({
        "organizationId": "00000000-0000-4000-8000-000000000001",
        "projectId": "00000000-0000-4000-8000-000000000002",
        "environmentId": "00000000-0000-4000-8000-000000000003",
        "profileId": "00000000-0000-4000-8000-000000000004",
        "revisionId": "00000000-0000-4000-8000-000000000005",
        "attemptId": "00000000-0000-4000-8000-000000000006",
        "requestDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "requestBodyBytes": 128,
        "state": "dispatching",
        "recoveryState": "indeterminate",
        "reservedAt": "2026-08-22T00:00:00Z",
        "leaseExpiresAt": "2026-08-22T00:00:30Z",
        "dispatchStartedAt": "2026-08-22T00:00:01Z",
        "outcomeDeadlineAt": "2026-08-22T00:00:11Z",
        "terminalAt": null,
        "createdAt": "2026-08-22T00:00:00Z",
        "observedAt": "2026-08-22T00:00:12Z",
        "evidenceOutcome": null,
        "responseStatus": null,
        "responseDigest": null,
        "responseBodyBytes": null,
        "retryAfterSeconds": null,
        "evidenceStartedAt": null,
        "evidenceCompletedAt": null
    })
}

fn connector_execution_attempt_resolution_example() -> Value {
    json!({
        "organizationId": "00000000-0000-4000-8000-000000000001",
        "projectId": "00000000-0000-4000-8000-000000000002",
        "environmentId": "00000000-0000-4000-8000-000000000003",
        "profileId": "00000000-0000-4000-8000-000000000004",
        "revisionId": "00000000-0000-4000-8000-000000000005",
        "attemptId": "00000000-0000-4000-8000-000000000006",
        "requestDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "requestBodyBytes": 128,
        "dispatchStartedAt": "2026-08-22T00:00:01Z",
        "outcomeDeadlineAt": "2026-08-22T00:00:11Z",
        "resolution": "indeterminate",
        "reason": "Provider outcome could not be established",
        "resolvedBy": "00000000-0000-4000-8000-000000000007",
        "resolvedAt": "2026-08-22T00:00:12Z"
    })
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
