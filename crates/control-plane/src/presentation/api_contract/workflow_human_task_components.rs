use super::workflow_components::{
    digest_schema, nullable_uuid_schema, revision_number_schema, timestamp_schema, uuid_schema,
};
use a3s_form_core::{
    DEFAULT_INTERACTION_MAX_VALUE_BYTES, FORM_INTERACTION_REQUEST_API_VERSION,
    FORM_RELEASE_REF_API_VERSION,
};
use serde_json::{json, Map, Value};

const MAX_EXTERNAL_IDENTITY_BYTES: usize = 512;
const MAX_INTERACTION_MESSAGE_BYTES: usize = 4_096;
const MAX_INTERACTION_DETAILS_BYTES: usize = 16 * 1_024;

pub(super) fn install_workflow_human_task_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert(
        "FormInteractionOutcome".into(),
        json!({ "type": "string", "enum": ["submit", "approve", "reject"] }),
    );
    schemas.insert("FormReleaseRef".into(), form_release_ref_schema());
    schemas.insert(
        "FormInteractionOutputMappingIdentity".into(),
        output_mapping_identity_schema(),
    );
    schemas.insert(
        "FormInteractionOutputMappingRegistry".into(),
        output_mapping_registry_schema(),
    );
    schemas.insert(
        "FormInteractionOutputMapping".into(),
        output_mapping_schema(),
    );
    schemas.insert(
        "WorkflowInteractionIdentity".into(),
        workflow_interaction_identity_schema(),
    );
    schemas.insert(
        "FormInteractionAssignment".into(),
        form_interaction_assignment_schema(),
    );
    schemas.insert(
        "FormInteractionTaskBinding".into(),
        form_interaction_task_binding_schema(),
    );
    schemas.insert(
        "FormInteractionRequest".into(),
        form_interaction_request_schema(),
    );
    schemas.insert(
        "HumanTaskAssignmentPolicy".into(),
        human_task_assignment_policy_schema(),
    );
    schemas.insert("HumanTaskSummary".into(), human_task_summary_schema());
    schemas.insert("HumanTask".into(), human_task_schema());
    schemas.insert(
        "HumanTaskList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/HumanTaskSummary" }
        }),
    );
    schemas.insert("HumanTaskMutation".into(), human_task_mutation_schema());
}

fn form_release_ref_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "apiVersion", "organizationId", "projectId", "formId", "releaseId", "uri",
            "revision", "digest", "compilerRevision", "schemaProfile", "mode"
        ],
        "properties": {
            "apiVersion": {
                "type": "string",
                "enum": [FORM_RELEASE_REF_API_VERSION]
            },
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "formId": uuid_schema(),
            "releaseId": uuid_schema(),
            "uri": {
                "type": "string",
                "format": "uri",
                "minLength": 1,
                "maxLength": MAX_EXTERNAL_IDENTITY_BYTES
            },
            "revision": revision_number_schema(),
            "digest": digest_schema(),
            "compilerRevision": external_identity_schema(),
            "schemaProfile": external_identity_schema(),
            "mode": { "type": "string", "enum": ["interaction"] }
        }
    })
}

fn output_mapping_identity_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind"],
        "properties": {
            "kind": { "type": "string", "enum": ["identity"] }
        }
    })
}

fn output_mapping_registry_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "registryKey", "revision", "digest"],
        "properties": {
            "kind": { "type": "string", "enum": ["registry"] },
            "registryKey": external_identity_schema(),
            "revision": revision_number_schema(),
            "digest": digest_schema()
        }
    })
}

fn output_mapping_schema() -> Value {
    json!({
        "oneOf": [
            { "$ref": "#/components/schemas/FormInteractionOutputMappingIdentity" },
            { "$ref": "#/components/schemas/FormInteractionOutputMappingRegistry" }
        ],
        "discriminator": {
            "propertyName": "kind",
            "mapping": {
                "identity": "#/components/schemas/FormInteractionOutputMappingIdentity",
                "registry": "#/components/schemas/FormInteractionOutputMappingRegistry"
            }
        }
    })
}

fn workflow_interaction_identity_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "workflowRunId", "flowRunId", "stepId", "stepAttempt", "humanTaskId", "flowHookId"
        ],
        "properties": {
            "workflowRunId": uuid_schema(),
            "flowRunId": external_identity_schema(),
            "stepId": external_identity_schema(),
            "stepAttempt": revision_number_schema(),
            "humanTaskId": uuid_schema(),
            "flowHookId": external_identity_schema()
        }
    })
}

fn form_interaction_assignment_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "policyId", "policyRevision", "policyDigest", "claimedPrincipalId"
        ],
        "properties": {
            "policyId": external_identity_schema(),
            "policyRevision": revision_number_schema(),
            "policyDigest": digest_schema(),
            "claimedPrincipalId": uuid_schema()
        }
    })
}

fn form_interaction_task_binding_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["version", "createdAt"],
        "properties": {
            "version": revision_number_schema(),
            "createdAt": timestamp_schema(),
            "dueAt": timestamp_schema(),
            "expiresAt": timestamp_schema()
        }
    })
}

fn form_interaction_request_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "apiVersion", "requestId", "identity", "form", "assignment", "task",
            "allowedOutcomes", "outputMapping", "maxValueBytes", "digest"
        ],
        "properties": {
            "apiVersion": {
                "type": "string",
                "enum": [FORM_INTERACTION_REQUEST_API_VERSION]
            },
            "requestId": external_identity_schema(),
            "identity": { "$ref": "#/components/schemas/WorkflowInteractionIdentity" },
            "form": { "$ref": "#/components/schemas/FormReleaseRef" },
            "assignment": { "$ref": "#/components/schemas/FormInteractionAssignment" },
            "task": { "$ref": "#/components/schemas/FormInteractionTaskBinding" },
            "allowedOutcomes": allowed_outcomes_schema(),
            "outputMapping": {
                "$ref": "#/components/schemas/FormInteractionOutputMapping"
            },
            "maxValueBytes": max_value_bytes_schema(),
            "initialValue": canonical_object_schema(false),
            "digest": digest_schema()
        }
    })
}

fn human_task_assignment_policy_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "revision", "digest"],
        "properties": {
            "id": external_identity_schema(),
            "revision": revision_number_schema(),
            "digest": digest_schema()
        }
    })
}

fn human_task_summary_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": human_task_summary_required_fields(),
        "properties": human_task_summary_properties()
    })
}

fn human_task_schema() -> Value {
    let mut properties = human_task_summary_properties();
    properties.insert(
        "details".into(),
        json!({
            "type": "string",
            "maxLength": MAX_INTERACTION_DETAILS_BYTES,
            "nullable": true
        }),
    );
    properties.insert(
        "outputMapping".into(),
        json!({ "$ref": "#/components/schemas/FormInteractionOutputMapping" }),
    );
    properties.insert("maxValueBytes".into(), max_value_bytes_schema());
    properties.insert("initialValue".into(), canonical_object_schema(true));
    properties.insert(
        "interactionRequest".into(),
        json!({
            "type": "object",
            "nullable": true,
            "allOf": [{ "$ref": "#/components/schemas/FormInteractionRequest" }]
        }),
    );
    let mut required = human_task_summary_required_fields();
    required.extend([
        "details",
        "outputMapping",
        "maxValueBytes",
        "initialValue",
        "interactionRequest",
    ]);
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn human_task_summary_required_fields() -> Vec<&'static str> {
    vec![
        "organizationId",
        "projectId",
        "id",
        "workflowRunId",
        "stepId",
        "stepAttempt",
        "formRelease",
        "assignmentPolicy",
        "status",
        "claimedBy",
        "decisionId",
        "aggregateVersion",
        "message",
        "allowedOutcomes",
        "createdAt",
        "updatedAt",
        "dueAt",
        "expiresAt",
        "claimedAt",
        "terminalAt",
    ]
}

fn human_task_summary_properties() -> Map<String, Value> {
    [
        ("organizationId", uuid_schema()),
        ("projectId", uuid_schema()),
        ("id", uuid_schema()),
        ("workflowRunId", uuid_schema()),
        ("stepId", external_identity_schema()),
        ("stepAttempt", revision_number_schema()),
        (
            "formRelease",
            json!({ "$ref": "#/components/schemas/FormReleaseRef" }),
        ),
        (
            "assignmentPolicy",
            json!({ "$ref": "#/components/schemas/HumanTaskAssignmentPolicy" }),
        ),
        (
            "status",
            json!({
                "type": "string",
                "enum": [
                    "pending_activation", "ready", "claimed", "completed", "expired", "cancelled"
                ]
            }),
        ),
        ("claimedBy", nullable_uuid_schema()),
        ("decisionId", nullable_uuid_schema()),
        ("aggregateVersion", revision_number_schema()),
        (
            "message",
            json!({
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_INTERACTION_MESSAGE_BYTES
            }),
        ),
        ("allowedOutcomes", allowed_outcomes_schema()),
        ("createdAt", timestamp_schema()),
        ("updatedAt", timestamp_schema()),
        ("dueAt", nullable_timestamp_schema()),
        ("expiresAt", nullable_timestamp_schema()),
        ("claimedAt", nullable_timestamp_schema()),
        ("terminalAt", nullable_timestamp_schema()),
    ]
    .into_iter()
    .map(|(name, schema)| (name.to_owned(), schema))
    .collect()
}

fn human_task_mutation_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["humanTask", "replayed"],
        "properties": {
            "humanTask": { "$ref": "#/components/schemas/HumanTask" },
            "replayed": { "type": "boolean" }
        }
    })
}

fn external_identity_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_EXTERNAL_IDENTITY_BYTES
    })
}

fn allowed_outcomes_schema() -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "maxItems": 3,
        "uniqueItems": true,
        "items": { "$ref": "#/components/schemas/FormInteractionOutcome" }
    })
}

fn max_value_bytes_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": DEFAULT_INTERACTION_MAX_VALUE_BYTES
    })
}

fn canonical_object_schema(nullable: bool) -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "nullable": nullable,
        "description": "Canonical JSON object governed by the referenced Form interaction contract."
    })
}

fn nullable_timestamp_schema() -> Value {
    json!({ "type": "string", "format": "date-time", "nullable": true })
}
