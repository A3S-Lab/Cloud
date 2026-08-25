use super::workflow_components::{
    digest_schema, nullable_digest_schema, timestamp_schema, uuid_schema,
};
use super::workflow_run_components::sequence_schema;
use crate::modules::workflow::{
    WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES, WORKFLOW_RUN_DIAGNOSTICS_SCHEMA,
    WORKFLOW_RUN_HISTORY_MAX_LIMIT, WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA,
};
use serde_json::{json, Map, Value};

pub(super) fn install_workflow_run_observation_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert(
        "WorkflowRunHistoryEvent".into(),
        workflow_run_history_event_schema(),
    );
    schemas.insert(
        "WorkflowRunHistoryPage".into(),
        workflow_run_history_page_schema(),
    );
    schemas.insert(
        "WorkflowRunDiagnostic".into(),
        workflow_run_diagnostic_schema(),
    );
    schemas.insert(
        "WorkflowRunStepStatistics".into(),
        workflow_run_step_statistics_schema(),
    );
    schemas.insert(
        "WorkflowRunFlowStatistics".into(),
        workflow_run_flow_statistics_schema(),
    );
    schemas.insert(
        "WorkflowRunEvidenceCorrelation".into(),
        workflow_run_evidence_correlation_schema(),
    );
    schemas.insert(
        "WorkflowRunDiagnostics".into(),
        workflow_run_diagnostics_schema(),
    );
    schemas.insert("WorkflowRunVariable".into(), workflow_run_variable_schema());
    schemas.insert(
        "WorkflowRunVariableInspection".into(),
        workflow_run_variable_inspection_schema(),
    );
}

fn workflow_run_history_event_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "sequence", "eventId", "eventKey", "occurredAt", "stepId", "attempt", "details"
        ],
        "properties": {
            "sequence": sequence_schema(),
            "eventId": uuid_schema(),
            "eventKey": {
                "type": "string",
                "minLength": 3,
                "pattern": "^[a-z][a-z0-9-]*(\\.[a-z][a-z0-9-]*){2,}$"
            },
            "occurredAt": timestamp_schema(),
            "stepId": nullable_string_schema(128),
            "attempt": nullable_u32_schema(),
            "details": {}
        }
    })
}

fn workflow_run_history_page_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["events", "nextSequence"],
        "properties": {
            "events": {
                "type": "array",
                "maxItems": WORKFLOW_RUN_HISTORY_MAX_LIMIT,
                "items": { "$ref": "#/components/schemas/WorkflowRunHistoryEvent" }
            },
            "nextSequence": nullable_sequence_schema()
        }
    })
}

fn workflow_run_diagnostic_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["code", "severity", "message"],
        "properties": {
            "code": {
                "type": "string",
                "enum": [
                    "flow_history_missing", "projection_lag", "projection_ahead",
                    "active_external_wait", "cancellation_pending", "retry_observed",
                    "runtime_recovery_observed", "step_failure_observed", "run_failed",
                    "run_timed_out", "run_cancelled"
                ]
            },
            "severity": { "type": "string", "enum": ["info", "warning", "error"] },
            "message": { "type": "string", "minLength": 1, "maxLength": 4_096 }
        }
    })
}

fn workflow_run_step_statistics_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "total", "pending", "running", "completed", "failed", "cancelled", "skipped",
            "totalAttemptGenerations", "evidenceReferenceCount"
        ],
        "properties": {
            "total": sequence_schema(),
            "pending": sequence_schema(),
            "running": sequence_schema(),
            "completed": sequence_schema(),
            "failed": sequence_schema(),
            "cancelled": sequence_schema(),
            "skipped": sequence_schema(),
            "totalAttemptGenerations": sequence_schema(),
            "evidenceReferenceCount": sequence_schema()
        }
    })
}

fn workflow_run_flow_statistics_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "eventCount", "eventCounts", "durableStepCount", "activeHookCount",
            "pendingTimerCount", "linkedChildOperationCount", "childWorkflowCount",
            "retryEventCount", "hostShutdownCount"
        ],
        "properties": {
            "eventCount": sequence_schema(),
            "eventCounts": {
                "type": "object",
                "additionalProperties": sequence_schema()
            },
            "durableStepCount": sequence_schema(),
            "activeHookCount": sequence_schema(),
            "pendingTimerCount": sequence_schema(),
            "linkedChildOperationCount": sequence_schema(),
            "childWorkflowCount": sequence_schema(),
            "retryEventCount": sequence_schema(),
            "hostShutdownCount": sequence_schema()
        }
    })
}

fn workflow_run_evidence_correlation_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["stepId", "references"],
        "properties": {
            "stepId": identifier_schema(),
            "references": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/WorkflowStepEvidenceReference" }
            }
        }
    })
}

fn workflow_run_diagnostics_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "workflowRunId", "operationId", "flowRunId", "runStatus",
            "observedFlowStatus", "flowRuntimeBuildId", "projectedFlowSequence",
            "observedFlowSequence", "unprojectedEventCount", "observedAt", "stepStatistics",
            "flowStatistics", "evidenceCorrelations", "evidenceCorrelationsTruncated",
            "diagnosticStatus", "diagnostics"
        ],
        "properties": {
            "schema": { "type": "string", "enum": [WORKFLOW_RUN_DIAGNOSTICS_SCHEMA] },
            "workflowRunId": uuid_schema(),
            "operationId": uuid_schema(),
            "flowRunId": { "type": "string", "minLength": 1 },
            "runStatus": { "$ref": "#/components/schemas/WorkflowRunStatus" },
            "observedFlowStatus": {
                "type": "string",
                "enum": [
                    "missing", "pending", "running", "suspended", "cancelling", "completed",
                    "failed", "cancelled", "continued_as_new"
                ]
            },
            "flowRuntimeBuildId": nullable_string_schema(255),
            "projectedFlowSequence": sequence_schema(),
            "observedFlowSequence": nullable_sequence_schema(),
            "unprojectedEventCount": sequence_schema(),
            "observedAt": timestamp_schema(),
            "stepStatistics": { "$ref": "#/components/schemas/WorkflowRunStepStatistics" },
            "flowStatistics": { "$ref": "#/components/schemas/WorkflowRunFlowStatistics" },
            "evidenceCorrelations": {
                "type": "array",
                "maxItems": WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES,
                "items": { "$ref": "#/components/schemas/WorkflowRunEvidenceCorrelation" }
            },
            "evidenceCorrelationsTruncated": { "type": "boolean" },
            "diagnosticStatus": {
                "type": "string",
                "enum": ["ok", "attention", "error"]
            },
            "diagnostics": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/WorkflowRunDiagnostic" }
            }
        }
    })
}

fn workflow_run_variable_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "name", "scope", "valueType", "valueSchemaDigest", "storageClass",
            "mutationMode", "required", "sourceStepId", "state", "redacted", "value",
            "valueDigest"
        ],
        "properties": {
            "name": identifier_schema(),
            "scope": {
                "type": "string",
                "enum": [
                    "invocation_input", "node_output", "composite_local", "run", "application"
                ]
            },
            "valueType": { "$ref": "#/components/schemas/WorkflowDataType" },
            "valueSchemaDigest": digest_schema(),
            "storageClass": {
                "type": "string",
                "enum": ["inline", "secret_reference", "immutable_object_reference"]
            },
            "mutationMode": {
                "type": "string",
                "enum": ["immutable", "deterministic", "optimistic_application_port"]
            },
            "required": { "type": "boolean" },
            "sourceStepId": nullable_string_schema(128),
            "state": { "type": "string", "enum": ["materialized", "unavailable"] },
            "redacted": { "type": "boolean" },
            "value": {},
            "valueDigest": nullable_digest_schema()
        }
    })
}

fn workflow_run_variable_inspection_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "workflowRunId", "planRevisionId", "variableContractDigest",
            "lastFlowSequence", "observedAt", "variables"
        ],
        "properties": {
            "schema": {
                "type": "string",
                "enum": [WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA]
            },
            "workflowRunId": uuid_schema(),
            "planRevisionId": uuid_schema(),
            "variableContractDigest": digest_schema(),
            "lastFlowSequence": sequence_schema(),
            "observedAt": timestamp_schema(),
            "variables": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/WorkflowRunVariable" }
            }
        }
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

fn nullable_u32_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "maximum": 4_294_967_295_u64,
        "nullable": true
    })
}

fn nullable_sequence_schema() -> Value {
    let mut schema = sequence_schema();
    schema["nullable"] = json!(true);
    schema
}
