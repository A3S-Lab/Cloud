use super::workflow_components::{digest_schema, timestamp_schema, uuid_schema};
use serde_json::{json, Map, Value};

const MAXIMUM_JSON_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) fn install_agent_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert("AgentConversation".into(), agent_conversation_schema());
    schemas.insert(
        "AgentConversationList".into(),
        array_schema("AgentConversation"),
    );
    schemas.insert(
        "AgentConversationMutation".into(),
        object_schema(
            &["conversation", "replayed"],
            json!({
                "conversation": schema_ref("AgentConversation"),
                "replayed": { "type": "boolean" }
            }),
        ),
    );
    schemas.insert("AgentReleaseBinding".into(), agent_release_binding_schema());
    schemas.insert(
        "AgentProviderProfile".into(),
        agent_provider_profile_schema(),
    );
    schemas.insert("AgentExecution".into(), agent_execution_schema());
    schemas.insert("AgentExecutionList".into(), array_schema("AgentExecution"));
    schemas.insert(
        "AgentExecutionMutation".into(),
        object_schema(
            &["conversation", "execution", "replayed"],
            json!({
                "conversation": schema_ref("AgentConversation"),
                "execution": schema_ref("AgentExecution"),
                "replayed": { "type": "boolean" }
            }),
        ),
    );
    schemas.insert(
        "AgentCodeRunIdentity".into(),
        agent_code_run_identity_schema(),
    );
    schemas.insert("AgentCodeChangeSet".into(), agent_code_change_set_schema());
    schemas.insert(
        "AgentExecutionChangeSet".into(),
        agent_execution_change_set_schema(),
    );
    schemas.insert("AgentExecutionEvent".into(), agent_execution_event_schema());
    schemas.insert(
        "AgentExecutionEventPage".into(),
        agent_execution_event_page_schema(),
    );
}

fn agent_conversation_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "environmentId",
            "id",
            "status",
            "lastEventSequence",
            "aggregateVersion",
            "createdAt",
            "updatedAt",
            "closedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "environmentId": uuid_schema(),
            "id": uuid_schema(),
            "status": { "type": "string", "enum": ["active", "closed"] },
            "lastEventSequence": nonnegative_sequence_schema(),
            "aggregateVersion": positive_sequence_schema(),
            "createdAt": timestamp_schema(),
            "updatedAt": timestamp_schema(),
            "closedAt": nullable_timestamp_schema()
        }),
    )
}

fn agent_release_binding_schema() -> Value {
    object_schema(
        &[
            "assetId",
            "assetReleaseId",
            "buildRunId",
            "artifactUri",
            "artifactDigest",
            "artifactMediaType",
            "artifactSizeBytes",
        ],
        json!({
            "assetId": uuid_schema(),
            "assetReleaseId": uuid_schema(),
            "buildRunId": uuid_schema(),
            "artifactUri": {
                "type": "string",
                "minLength": 1,
                "maxLength": 2048,
                "pattern": "^oci://.+@sha256:[0-9a-f]{64}$"
            },
            "artifactDigest": digest_schema(),
            "artifactMediaType": {
                "type": "string",
                "minLength": 1,
                "maxLength": 255,
                "pattern": "^[^\\u0000\\r\\n]+$"
            },
            "artifactSizeBytes": positive_sequence_schema()
        }),
    )
}

fn agent_provider_profile_schema() -> Value {
    object_schema(
        &[
            "kind",
            "revision",
            "protocol",
            "nativeProtocol",
            "profileDigest",
            "capabilityDigest",
        ],
        json!({
            "kind": {
                "type": "string",
                "enum": ["a3s.code", "reference.echo"]
            },
            "revision": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[A-Za-z0-9](?:[A-Za-z0-9.+-]*[A-Za-z0-9])?$"
            },
            "protocol": {
                "type": "string",
                "enum": ["a3s.cloud.agent-provider.v1"]
            },
            "nativeProtocol": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[a-z0-9]+(?:[.-][a-z0-9]+)*$"
            },
            "profileDigest": digest_schema(),
            "capabilityDigest": digest_schema()
        }),
    )
}

fn agent_execution_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "conversationId",
            "id",
            "operationId",
            "agent",
            "provider",
            "status",
            "failure",
            "aggregateVersion",
            "requestedAt",
            "updatedAt",
            "startedAt",
            "cancellationRequestedAt",
            "finishedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "conversationId": uuid_schema(),
            "id": uuid_schema(),
            "operationId": uuid_schema(),
            "agent": schema_ref("AgentReleaseBinding"),
            "provider": schema_ref("AgentProviderProfile"),
            "status": {
                "type": "string",
                "enum": ["pending", "running", "cancelling", "succeeded", "failed", "cancelled"]
            },
            "failure": {
                "type": "string",
                "minLength": 1,
                "maxLength": 16384,
                "x-a3s-max-utf8-bytes": 16384,
                "nullable": true
            },
            "aggregateVersion": positive_sequence_schema(),
            "requestedAt": timestamp_schema(),
            "updatedAt": timestamp_schema(),
            "startedAt": nullable_timestamp_schema(),
            "cancellationRequestedAt": nullable_timestamp_schema(),
            "finishedAt": nullable_timestamp_schema()
        }),
    )
}

fn agent_code_run_identity_schema() -> Value {
    object_schema(
        &[
            "schema",
            "protocol",
            "agent_release_identity",
            "session_id",
            "run_id",
        ],
        json!({
            "schema": { "type": "string", "enum": ["a3s.code.agent-run-identity.v1"] },
            "protocol": { "type": "string", "enum": ["a3s.code.agent.v1"] },
            "agent_release_identity": digest_schema(),
            "session_id": bounded_line_schema(256),
            "run_id": bounded_line_schema(256)
        }),
    )
}

fn agent_code_change_set_schema() -> Value {
    object_schema(
        &[
            "schema",
            "identity",
            "state",
            "format",
            "encoding",
            "base_tree",
            "result_tree",
            "patch_digest",
            "patch_bytes",
            "patch_base64",
            "observed_at_ms",
        ],
        json!({
            "schema": { "type": "string", "enum": ["a3s.code.agent-change-set.v1"] },
            "identity": schema_ref("AgentCodeRunIdentity"),
            "state": { "type": "string", "enum": ["completed", "failed", "cancelled"] },
            "format": { "type": "string", "enum": ["git_unified_diff_v1"] },
            "encoding": { "type": "string", "enum": ["base64"] },
            "base_tree": { "type": "string", "pattern": "^git-tree:(?:[0-9a-f]{40}|[0-9a-f]{64})$" },
            "result_tree": { "type": "string", "pattern": "^git-tree:(?:[0-9a-f]{40}|[0-9a-f]{64})$" },
            "patch_digest": digest_schema(),
            "patch_bytes": {
                "type": "integer",
                "format": "int64",
                "minimum": 0,
                "maximum": 4194304
            },
            "patch_base64": {
                "type": "string",
                "format": "byte",
                "maxLength": 5592408,
                "x-a3s-decoded-max-bytes": 4194304
            },
            "observed_at_ms": positive_sequence_schema()
        }),
    )
}

fn agent_execution_change_set_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "executionId",
            "batchId",
            "nodeId",
            "changeSet",
            "recordedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "executionId": uuid_schema(),
            "batchId": uuid_schema(),
            "nodeId": uuid_schema(),
            "changeSet": schema_ref("AgentCodeChangeSet"),
            "recordedAt": timestamp_schema()
        }),
    )
}

fn agent_execution_event_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "conversationId",
            "executionId",
            "sequence",
            "kind",
            "content",
            "contentDigest",
            "contentSizeBytes",
            "occurredAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "conversationId": uuid_schema(),
            "executionId": uuid_schema(),
            "sequence": positive_sequence_schema(),
            "kind": {
                "type": "string",
                "enum": [
                    "execution_requested",
                    "model_output",
                    "execution_failed",
                    "execution_completed",
                    "execution_cancelled"
                ]
            },
            "content": {
                "description": "Bounded semantic JSON content whose SHA-256 identity and encoded byte length are carried alongside it."
            },
            "contentDigest": digest_schema(),
            "contentSizeBytes": {
                "type": "integer",
                "format": "int64",
                "minimum": 1,
                "maximum": 65536
            },
            "occurredAt": timestamp_schema()
        }),
    )
}

fn agent_execution_event_page_schema() -> Value {
    object_schema(
        &["conversationId", "headSequence", "records", "nextCursor"],
        json!({
            "conversationId": uuid_schema(),
            "headSequence": nonnegative_sequence_schema(),
            "records": {
                "type": "array",
                "maxItems": 200,
                "items": schema_ref("AgentExecutionEvent")
            },
            "nextCursor": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "nullable": true
            }
        }),
    )
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn array_schema(item: &str) -> Value {
    json!({
        "type": "array",
        "maxItems": 200,
        "items": schema_ref(item)
    })
}

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn nonnegative_sequence_schema() -> Value {
    json!({
        "type": "integer",
        "format": "int64",
        "minimum": 0,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

fn positive_sequence_schema() -> Value {
    json!({
        "type": "integer",
        "format": "int64",
        "minimum": 1,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

fn nullable_timestamp_schema() -> Value {
    json!({ "type": "string", "format": "date-time", "nullable": true })
}

fn bounded_line_schema(max_length: usize) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": max_length,
        "pattern": "^[^\\u0000\\r\\n]+$"
    })
}
