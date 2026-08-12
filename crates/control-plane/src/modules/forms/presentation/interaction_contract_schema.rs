use serde_json::{json, Value};

pub(crate) fn form_interaction_submission_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "apiVersion",
            "submissionId",
            "requestId",
            "requestDigest",
            "identity",
            "form",
            "assignment",
            "taskVersion",
            "principalId",
            "outcome",
            "idempotencyKey",
            "submittedAt",
            "value",
            "valueDigest"
        ],
        "properties": {
            "apiVersion": {
                "type": "string",
                "enum": ["a3s.dev/form-interaction-submission/v1"]
            },
            "submissionId": {"type": "string", "format": "uuid"},
            "requestId": {"type": "string", "minLength": 1, "maxLength": 512},
            "requestDigest": digest_schema(),
            "identity": workflow_interaction_identity_schema(),
            "form": form_release_ref_schema(),
            "assignment": submission_assignment_schema(),
            "taskVersion": revision_schema(),
            "principalId": {"type": "string", "format": "uuid"},
            "outcome": {"type": "string", "enum": ["submit", "approve", "reject"]},
            "idempotencyKey": {
                "type": "string",
                "minLength": 1,
                "maxLength": 255,
                "pattern": "^[A-Za-z0-9._~:/-]+$"
            },
            "submittedAt": {"type": "string", "format": "date-time"},
            "value": {
                "type": "object",
                "description": "Canonical JSON evaluated by the exact native A3S Form release."
            },
            "valueDigest": digest_schema()
        }
    })
}

fn workflow_interaction_identity_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "workflowRunId",
            "flowRunId",
            "stepId",
            "stepAttempt",
            "humanTaskId",
            "flowHookId"
        ],
        "properties": {
            "workflowRunId": {"type": "string", "format": "uuid"},
            "flowRunId": {"type": "string", "minLength": 1, "maxLength": 512},
            "stepId": {"type": "string", "minLength": 1, "maxLength": 512},
            "stepAttempt": revision_schema(),
            "humanTaskId": {"type": "string", "format": "uuid"},
            "flowHookId": {"type": "string", "minLength": 1, "maxLength": 512}
        }
    })
}

fn form_release_ref_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "apiVersion",
            "organizationId",
            "projectId",
            "formId",
            "releaseId",
            "uri",
            "revision",
            "digest",
            "compilerRevision",
            "schemaProfile",
            "mode"
        ],
        "properties": {
            "apiVersion": {"type": "string", "enum": ["a3s.dev/form-release-ref/v1"]},
            "organizationId": {"type": "string", "format": "uuid"},
            "projectId": {"type": "string", "format": "uuid"},
            "formId": {"type": "string", "minLength": 1, "maxLength": 512},
            "releaseId": {"type": "string", "minLength": 1, "maxLength": 512},
            "uri": {"type": "string", "minLength": 1, "maxLength": 512, "format": "uri"},
            "revision": revision_schema(),
            "digest": digest_schema(),
            "compilerRevision": {"type": "string", "minLength": 1, "maxLength": 512},
            "schemaProfile": {"type": "string", "minLength": 1, "maxLength": 512},
            "mode": {"type": "string", "enum": ["interaction"]}
        }
    })
}

fn submission_assignment_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["policyId", "policyRevision", "policyDigest"],
        "properties": {
            "policyId": {"type": "string", "minLength": 1, "maxLength": 512},
            "policyRevision": revision_schema(),
            "policyDigest": digest_schema()
        }
    })
}

fn digest_schema() -> Value {
    json!({"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"})
}

fn revision_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": 9_007_199_254_740_991_u64
    })
}
