use super::workflow_components::{
    digest_schema, nullable_digest_schema, nullable_uuid_schema, revision_number_schema,
    timestamp_schema, uuid_schema, MAXIMUM_JSON_SAFE_INTEGER,
};
use crate::modules::workflow::domain::{
    WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES, WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
};
use crate::modules::workflow::{
    WORKFLOW_EXECUTION_RESULT_SCHEMA, WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V4,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V5, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V6,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V7, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V8,
};
use serde_json::{json, Map, Value};

pub(super) fn install_workflow_run_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert("WorkflowRunStatus".into(), workflow_run_status_schema());
    install_execution_outcome_schemas(schemas);
    schemas.insert(
        "WorkflowExecutionStepOutput".into(),
        workflow_execution_step_output_schema(),
    );
    schemas.insert(
        "WorkflowExecutionFailureDetails".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["kind", "output"],
            "properties": {
                "kind": { "type": "string", "enum": ["execution"] },
                "output": { "$ref": "#/components/schemas/WorkflowExecutionStepOutput" }
            }
        }),
    );
    schemas.insert(
        "WorkflowStepFailureOutput".into(),
        workflow_step_failure_output_schema(),
    );
    schemas.insert(
        "WorkflowStepDefaultOutputEvidence".into(),
        workflow_step_default_output_evidence_schema(),
    );
    schemas.insert(
        "WorkflowStepEvidenceReference".into(),
        workflow_step_evidence_reference_schema(),
    );
    schemas.insert(
        "WorkflowStepProjection".into(),
        workflow_step_projection_schema(),
    );
    schemas.insert("WorkflowRun".into(), workflow_run_schema());
    schemas.insert(
        "WorkflowRunList".into(),
        json!({
            "type": "array",
            "maxItems": 200,
            "items": { "$ref": "#/components/schemas/WorkflowRun" }
        }),
    );
    schemas.insert(
        "WorkflowRunMutation".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["workflowRun", "replayed"],
            "properties": {
                "workflowRun": { "$ref": "#/components/schemas/WorkflowRun" },
                "replayed": { "type": "boolean" }
            }
        }),
    );
    schemas.insert("WorkflowRunOutput".into(), workflow_run_output_schema());
}

fn install_execution_outcome_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert(
        "WorkflowExecutionSucceededOutcome".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["kind", "exit_code"],
            "properties": {
                "kind": { "type": "string", "enum": ["succeeded"] },
                "exit_code": { "type": "integer", "enum": [0] }
            }
        }),
    );
    schemas.insert(
        "WorkflowExecutionFailedOutcome".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["kind", "exit_code", "reason"],
            "properties": {
                "kind": { "type": "string", "enum": ["failed"] },
                "exit_code": {
                    "type": "integer",
                    "minimum": -2_147_483_648_i64,
                    "maximum": 2_147_483_647_i64,
                    "nullable": true
                },
                "reason": { "type": "string", "minLength": 1, "maxLength": 16_384 }
            }
        }),
    );
    schemas.insert(
        "WorkflowExecutionCancelledOutcome".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["kind"],
            "properties": {
                "kind": { "type": "string", "enum": ["cancelled"] }
            }
        }),
    );
    schemas.insert(
        "WorkflowExecutionOutcome".into(),
        json!({
            "oneOf": [
                { "$ref": "#/components/schemas/WorkflowExecutionSucceededOutcome" },
                { "$ref": "#/components/schemas/WorkflowExecutionFailedOutcome" },
                { "$ref": "#/components/schemas/WorkflowExecutionCancelledOutcome" }
            ],
            "discriminator": {
                "propertyName": "kind",
                "mapping": {
                    "succeeded": "#/components/schemas/WorkflowExecutionSucceededOutcome",
                    "failed": "#/components/schemas/WorkflowExecutionFailedOutcome",
                    "cancelled": "#/components/schemas/WorkflowExecutionCancelledOutcome"
                }
            }
        }),
    );
}

fn workflow_execution_step_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "executionId", "operationId", "executionTemplateId",
            "executionTemplateRevisionId", "executionTemplateDigest", "invocationTemplateDigest",
            "outcome", "finishedAt"
        ],
        "properties": {
            "schema": { "type": "string", "enum": [WORKFLOW_EXECUTION_RESULT_SCHEMA] },
            "executionId": uuid_schema(),
            "operationId": uuid_schema(),
            "executionTemplateId": uuid_schema(),
            "executionTemplateRevisionId": uuid_schema(),
            "executionTemplateDigest": digest_schema(),
            "invocationTemplateDigest": digest_schema(),
            "outcome": { "$ref": "#/components/schemas/WorkflowExecutionOutcome" },
            "finishedAt": timestamp_schema()
        }
    })
}

fn workflow_step_failure_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["schema", "stepId", "classification", "message"],
        "properties": {
            "schema": {
                "type": "string",
                "enum": [
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V4,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V5,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V6,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V7,
                    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V8
                ]
            },
            "stepId": identifier_schema(),
            "classification": {
                "type": "string",
                "enum": [
                    "dispatch_rejected", "execution_failed", "execution_cancelled",
                    "provider_rejected", "provider_attempts_exhausted", "provider_indeterminate",
                    "provider_observation_limit", "provider_response_invalid",
                    "application_invalid", "application_not_found", "application_conflict",
                    "application_forbidden", "workflow_local_invalid"
                ]
            },
            "message": { "type": "string", "minLength": 1, "maxLength": 16_384 },
            "details": { "$ref": "#/components/schemas/WorkflowExecutionFailureDetails" }
        }
    })
}

fn workflow_step_default_output_evidence_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["schema", "policyDigest", "port", "failure"],
        "properties": {
            "schema": {
                "type": "string",
                "enum": [WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA]
            },
            "policyDigest": digest_schema(),
            "port": identifier_schema(),
            "failure": { "$ref": "#/components/schemas/WorkflowStepFailureOutput" }
        }
    })
}

fn workflow_step_evidence_reference_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES,
        "pattern": "^urn:a3s:cloud:(connectors:attempt|executions:execution|forms:submission|operations:operation|workflow:human-task|workflow:workflow-decision|workflow:workflow-run):[0-9a-fA-F-]{36}$"
    })
}

fn workflow_step_projection_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "stepId", "kind", "status", "flowStepId", "attemptGeneration", "selectedHandle",
            "result", "resultDigest", "error", "defaultOutputEvidence", "evidenceReferences",
            "lastFlowSequence", "updatedAt"
        ],
        "properties": {
            "stepId": identifier_schema(),
            "kind": { "$ref": "#/components/schemas/WorkflowStepKind" },
            "status": {
                "type": "string",
                "enum": ["pending", "running", "completed", "failed", "cancelled", "skipped"]
            },
            "flowStepId": { "type": "string", "minLength": 1, "maxLength": 137 },
            "attemptGeneration": {
                "type": "integer",
                "minimum": 0,
                "maximum": 4_294_967_295_u64
            },
            "selectedHandle": nullable_string_schema(128),
            "result": {},
            "resultDigest": nullable_digest_schema(),
            "error": nullable_string_schema(16_384),
            "defaultOutputEvidence": nullable_ref(
                "#/components/schemas/WorkflowStepDefaultOutputEvidence"
            ),
            "evidenceReferences": {
                "type": "array",
                "maxItems": WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
                "items": { "$ref": "#/components/schemas/WorkflowStepEvidenceReference" }
            },
            "lastFlowSequence": sequence_schema(),
            "updatedAt": timestamp_schema()
        }
    })
}

fn workflow_run_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "id", "workflowGoalId", "planRevisionId",
            "planDigest", "operationId", "flowRunId", "flowRuntimeBuildId",
            "executionInputDigest", "status", "lastFlowSequence", "outputDigest", "error",
            "aggregateVersion", "requestedBy", "requestedAt", "updatedAt", "startedAt",
            "deadlineAt", "cancellationRequestedAt", "cancellationRequestedBy",
            "cancellationReason", "finishedAt", "steps"
        ],
        "properties": {
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "id": uuid_schema(),
            "workflowGoalId": uuid_schema(),
            "planRevisionId": uuid_schema(),
            "planDigest": digest_schema(),
            "operationId": uuid_schema(),
            "flowRunId": { "type": "string", "minLength": 1 },
            "flowRuntimeBuildId": nullable_string_schema(255),
            "executionInputDigest": digest_schema(),
            "status": { "$ref": "#/components/schemas/WorkflowRunStatus" },
            "lastFlowSequence": sequence_schema(),
            "outputDigest": nullable_digest_schema(),
            "error": nullable_string_schema(16_384),
            "aggregateVersion": revision_number_schema(),
            "requestedBy": uuid_schema(),
            "requestedAt": timestamp_schema(),
            "updatedAt": timestamp_schema(),
            "startedAt": nullable_timestamp_schema(),
            "deadlineAt": timestamp_schema(),
            "cancellationRequestedAt": nullable_timestamp_schema(),
            "cancellationRequestedBy": nullable_uuid_schema(),
            "cancellationReason": nullable_string_schema(4_096),
            "finishedAt": nullable_timestamp_schema(),
            "steps": {
                "type": "array",
                "maxItems": 10_000,
                "items": { "$ref": "#/components/schemas/WorkflowStepProjection" }
            }
        }
    })
}

fn workflow_run_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["workflowRunId", "output", "outputDigest", "finishedAt"],
        "properties": {
            "workflowRunId": uuid_schema(),
            "output": {},
            "outputDigest": digest_schema(),
            "finishedAt": timestamp_schema()
        }
    })
}

pub(super) fn workflow_run_status_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            "pending", "running", "waiting", "cancelling", "completed", "failed",
            "cancelled", "timed_out"
        ]
    })
}

fn identifier_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 128,
        "pattern": "^[A-Za-z][A-Za-z0-9_-]*$"
    })
}

fn nullable_string_schema(max_length: usize) -> Value {
    json!({
        "type": "string",
        "maxLength": max_length,
        "nullable": true
    })
}

fn nullable_timestamp_schema() -> Value {
    json!({ "type": "string", "format": "date-time", "nullable": true })
}

fn nullable_ref(reference: &str) -> Value {
    json!({
        "allOf": [{ "$ref": reference }],
        "nullable": true
    })
}

pub(super) fn sequence_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}
