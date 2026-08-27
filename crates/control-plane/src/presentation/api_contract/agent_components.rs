use super::workflow_components::{digest_schema, timestamp_schema, uuid_schema};
use a3s_cloud_contracts::HARNESS_INVOCATION_PROFILE_MAX_BYTES;
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
    schemas.insert(
        "HarnessAgentReleaseBinding".into(),
        harness_agent_release_binding_schema(),
    );
    schemas.insert(
        "HarnessProviderBinding".into(),
        harness_provider_binding_schema(),
    );
    schemas.insert(
        "HarnessWorkspaceBinding".into(),
        harness_workspace_binding_schema(),
    );
    schemas.insert("HarnessSkillBinding".into(), harness_skill_binding_schema());
    schemas.insert("HarnessMcpBinding".into(), harness_mcp_binding_schema());
    schemas.insert("HarnessModelBinding".into(), harness_model_binding_schema());
    schemas.insert(
        "HarnessSecretReference".into(),
        harness_secret_reference_schema(),
    );
    schemas.insert("HarnessToolBinding".into(), harness_tool_binding_schema());
    schemas.insert(
        "HarnessInvocationProfile".into(),
        harness_invocation_profile_schema(),
    );
    schemas.insert(
        "AgentToolPayloadIdentity".into(),
        agent_tool_payload_identity_schema(),
    );
    schemas.insert(
        "AgentModelOutputEventContent".into(),
        agent_model_output_event_content_schema(),
    );
    schemas.insert(
        "AgentToolRequestEventContent".into(),
        agent_tool_request_event_content_schema(),
    );
    schemas.insert(
        "AgentToolResultEventContent".into(),
        agent_tool_result_event_content_schema(),
    );
    schemas.insert(
        "AgentExecutionFailureEventContent".into(),
        agent_execution_failure_event_content_schema(),
    );
    schemas.insert(
        "AgentApprovalResolutionEventContent".into(),
        agent_approval_resolution_event_content_schema(),
    );
    for (name, kind, content) in [
        (
            "AgentExecutionRequestedEvent",
            "execution_requested",
            json!({
                "description": "Caller-owned execution input represented as bounded canonical JSON.",
                "x-a3s-max-canonical-bytes": 65536
            }),
        ),
        (
            "AgentModelOutputEvent",
            "model_output",
            schema_ref("AgentModelOutputEventContent"),
        ),
        (
            "AgentToolRequestEvent",
            "tool_request",
            schema_ref("AgentToolRequestEventContent"),
        ),
        (
            "AgentToolResultEvent",
            "tool_result",
            schema_ref("AgentToolResultEventContent"),
        ),
        (
            "AgentApprovalResolvedEvent",
            "approval_resolved",
            schema_ref("AgentApprovalResolutionEventContent"),
        ),
        (
            "AgentExecutionFailedEvent",
            "execution_failed",
            schema_ref("AgentExecutionFailureEventContent"),
        ),
        (
            "AgentExecutionCompletedEvent",
            "execution_completed",
            empty_event_content_schema(),
        ),
        (
            "AgentExecutionCancelledEvent",
            "execution_cancelled",
            empty_event_content_schema(),
        ),
    ] {
        schemas.insert(
            name.into(),
            agent_execution_event_variant_schema(kind, content),
        );
    }
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
    schemas.insert(
        "AgentApprovalCheckpoint".into(),
        agent_approval_checkpoint_schema(),
    );
    schemas.insert(
        "AgentApprovalCheckpointList".into(),
        bounded_array_schema("AgentApprovalCheckpoint", 1_000),
    );
    schemas.insert(
        "AgentApprovalCheckpointMutation".into(),
        object_schema(
            &["checkpoint", "replayed"],
            json!({
                "checkpoint": schema_ref("AgentApprovalCheckpoint"),
                "replayed": { "type": "boolean" }
            }),
        ),
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

fn harness_agent_release_binding_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "assetId",
            "assetReleaseId",
            "buildRunId",
            "artifactDigest",
        ],
        json!({
            "organizationId": uuid_schema(),
            "assetId": uuid_schema(),
            "assetReleaseId": uuid_schema(),
            "buildRunId": uuid_schema(),
            "artifactDigest": digest_schema()
        }),
    )
}

fn harness_provider_binding_schema() -> Value {
    object_schema(
        &["kind", "revision", "profileDigest", "capabilityDigest"],
        json!({
            "kind": {
                "type": "string",
                "enum": ["a3s.code", "reference.echo"]
            },
            "revision": bounded_line_schema(128),
            "profileDigest": digest_schema(),
            "capabilityDigest": digest_schema()
        }),
    )
}

fn harness_workspace_binding_schema() -> Value {
    object_schema(
        &[
            "workloadId",
            "workloadRevisionId",
            "runtimeUnitId",
            "runtimeGeneration",
            "runtimeSpecDigest",
            "workingDirectory",
        ],
        json!({
            "workloadId": uuid_schema(),
            "workloadRevisionId": uuid_schema(),
            "runtimeUnitId": bounded_line_schema(512),
            "runtimeGeneration": positive_sequence_schema(),
            "runtimeSpecDigest": digest_schema(),
            "workingDirectory": {
                "type": "string",
                "minLength": 1,
                "maxLength": 4096,
                "pattern": "^[^\\u0000\\r\\n]+$",
                "nullable": true
            }
        }),
    )
}

fn harness_skill_binding_schema() -> Value {
    object_schema(
        &["assetId", "assetReleaseId", "artifactDigest"],
        json!({
            "assetId": uuid_schema(),
            "assetReleaseId": uuid_schema(),
            "artifactDigest": digest_schema()
        }),
    )
}

fn harness_mcp_binding_schema() -> Value {
    object_schema(
        &["assetId", "assetReleaseId", "profileDigest"],
        json!({
            "assetId": uuid_schema(),
            "assetReleaseId": uuid_schema(),
            "profileDigest": digest_schema()
        }),
    )
}

fn harness_model_binding_schema() -> Value {
    object_schema(
        &["modelId", "modelRevisionId", "profileDigest"],
        json!({
            "modelId": uuid_schema(),
            "modelRevisionId": uuid_schema(),
            "profileDigest": digest_schema()
        }),
    )
}

fn harness_secret_reference_schema() -> Value {
    object_schema(
        &["name", "secretId", "version", "target"],
        json!({
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 63,
                "pattern": "^[A-Za-z0-9._-]+$"
            },
            "secretId": uuid_schema(),
            "version": positive_sequence_schema(),
            "target": {
                "oneOf": [
                    object_schema(
                        &["kind", "variable"],
                        json!({
                            "kind": { "type": "string", "enum": ["environment"] },
                            "variable": {
                                "type": "string",
                                "minLength": 1,
                                "maxLength": 255,
                                "pattern": "^[A-Z_][A-Z0-9_]*$"
                            }
                        })
                    ),
                    object_schema(
                        &["kind", "path", "mode"],
                        json!({
                            "kind": { "type": "string", "enum": ["file"] },
                            "path": bounded_line_schema(4096),
                            "mode": { "type": "integer", "minimum": 1, "maximum": 511 }
                        })
                    ),
                    object_schema(
                        &["kind"],
                        json!({
                            "kind": { "type": "string", "enum": ["registry_credential"] }
                        })
                    )
                ]
            }
        }),
    )
}

fn harness_tool_binding_schema() -> Value {
    object_schema(
        &["name", "revision", "contractDigest", "approvalRequired"],
        json!({
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[a-z0-9]+(?:[.-][a-z0-9]+)*$"
            },
            "revision": bounded_line_schema(128),
            "contractDigest": digest_schema(),
            "approvalRequired": { "type": "boolean" }
        }),
    )
}

fn harness_invocation_profile_schema() -> Value {
    let mut schema = object_schema(
        &[
            "schema",
            "agent",
            "provider",
            "instructionsDigest",
            "environmentPolicyDigest",
            "securityPolicyDigest",
            "workspace",
            "skills",
            "mcpServers",
            "models",
            "secrets",
            "tools",
            "requiredCapabilities",
        ],
        json!({
            "schema": {
                "type": "string",
                "enum": ["a3s.cloud.harness-invocation-profile.v1"]
            },
            "agent": schema_ref("HarnessAgentReleaseBinding"),
            "provider": schema_ref("HarnessProviderBinding"),
            "instructionsDigest": digest_schema(),
            "environmentPolicyDigest": digest_schema(),
            "securityPolicyDigest": digest_schema(),
            "workspace": schema_ref("HarnessWorkspaceBinding"),
            "skills": bounded_binding_array("HarnessSkillBinding"),
            "mcpServers": bounded_binding_array("HarnessMcpBinding"),
            "models": bounded_binding_array("HarnessModelBinding"),
            "secrets": bounded_binding_array("HarnessSecretReference"),
            "tools": bounded_binding_array("HarnessToolBinding"),
            "requiredCapabilities": {
                "type": "array",
                "minItems": 3,
                "maxItems": 32,
                "uniqueItems": true,
                "x-a3s-canonical-order": "lexical-wire-value",
                "items": {
                    "type": "string",
                    "enum": [
                        "cancellation", "change_set", "checkpoints", "cleanup",
                        "event_pages", "pause_resume", "recovery",
                        "streaming_output", "tool_calls"
                    ]
                }
            }
        }),
    );
    schema["x-a3s-max-canonical-bytes"] = json!(HARNESS_INVOCATION_PROFILE_MAX_BYTES);
    schema
}

fn agent_tool_payload_identity_schema() -> Value {
    object_schema(
        &["digest", "sizeBytes", "mediaType"],
        json!({
            "digest": digest_schema(),
            "sizeBytes": {
                "type": "integer",
                "format": "int64",
                "minimum": 0,
                "maximum": MAXIMUM_JSON_SAFE_INTEGER
            },
            "mediaType": bounded_line_schema(255)
        }),
    )
}

fn agent_model_output_event_content_schema() -> Value {
    object_schema(
        &["text"],
        json!({
            "text": {
                "type": "string",
                "minLength": 1,
                "maxLength": 65536,
                "x-a3s-max-utf8-bytes": 65536
            }
        }),
    )
}

fn agent_tool_request_event_content_schema() -> Value {
    object_schema(
        &["callId", "tool", "request"],
        json!({
            "callId": bounded_line_schema(256),
            "tool": schema_ref("HarnessToolBinding"),
            "request": schema_ref("AgentToolPayloadIdentity")
        }),
    )
}

fn agent_tool_result_event_content_schema() -> Value {
    object_schema(
        &["callId", "tool", "requestDigest", "outcome", "result"],
        json!({
            "callId": bounded_line_schema(256),
            "tool": schema_ref("HarnessToolBinding"),
            "requestDigest": digest_schema(),
            "outcome": { "type": "string", "enum": ["succeeded", "failed"] },
            "result": schema_ref("AgentToolPayloadIdentity")
        }),
    )
}

fn agent_execution_failure_event_content_schema() -> Value {
    object_schema(
        &["reason"],
        json!({
            "reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": 16384,
                "x-a3s-max-utf8-bytes": 16384
            }
        }),
    )
}

fn agent_approval_resolution_event_content_schema() -> Value {
    object_schema(
        &[
            "checkpointId",
            "decisionId",
            "outcome",
            "decisionDigest",
            "decidedBy",
            "authorizationDecision",
            "reason",
        ],
        json!({
            "checkpointId": uuid_schema(),
            "decisionId": uuid_schema(),
            "outcome": {
                "type": "string",
                "enum": ["approved", "denied", "expired"]
            },
            "decisionDigest": digest_schema(),
            "decidedBy": nullable_uuid_schema(),
            "authorizationDecision": {
                "type": "object",
                "additionalProperties": false,
                "required": ["id", "digest"],
                "nullable": true,
                "properties": {
                    "id": bounded_utf8_line_schema(512),
                    "digest": digest_schema()
                }
            },
            "reason": nullable_bounded_utf8_line_schema(1024)
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
            "invocationProfile",
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
            "invocationProfile": {
                "allOf": [schema_ref("HarnessInvocationProfile")],
                "nullable": true
            },
            "status": {
                "type": "string",
                "enum": [
                    "pending", "running", "awaiting_approval", "cancelling",
                    "succeeded", "failed", "cancelled"
                ]
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
    let mut schema = object_schema(
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
                    "tool_request",
                    "tool_result",
                    "approval_resolved",
                    "execution_failed",
                    "execution_completed",
                    "execution_cancelled"
                ]
            },
            "content": {
                "description": "Bounded semantic JSON content validated by the selected event variant."
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
    );
    schema["oneOf"] = json!([
        schema_ref("AgentExecutionRequestedEvent"),
        schema_ref("AgentModelOutputEvent"),
        schema_ref("AgentToolRequestEvent"),
        schema_ref("AgentToolResultEvent"),
        schema_ref("AgentApprovalResolvedEvent"),
        schema_ref("AgentExecutionFailedEvent"),
        schema_ref("AgentExecutionCompletedEvent"),
        schema_ref("AgentExecutionCancelledEvent")
    ]);
    schema["discriminator"] = json!({
        "propertyName": "kind",
        "mapping": {
            "execution_requested": "#/components/schemas/AgentExecutionRequestedEvent",
            "model_output": "#/components/schemas/AgentModelOutputEvent",
            "tool_request": "#/components/schemas/AgentToolRequestEvent",
            "tool_result": "#/components/schemas/AgentToolResultEvent",
            "approval_resolved": "#/components/schemas/AgentApprovalResolvedEvent",
            "execution_failed": "#/components/schemas/AgentExecutionFailedEvent",
            "execution_completed": "#/components/schemas/AgentExecutionCompletedEvent",
            "execution_cancelled": "#/components/schemas/AgentExecutionCancelledEvent"
        }
    });
    schema
}

fn agent_execution_event_variant_schema(kind: &str, content: Value) -> Value {
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
            "kind": { "type": "string", "enum": [kind] },
            "content": content,
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

fn empty_event_content_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "Empty terminal event content."
    })
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

fn agent_approval_checkpoint_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "environmentId",
            "conversationId",
            "executionId",
            "id",
            "providerRunIdentityDigest",
            "invocationProfileDigest",
            "sourceEventSequence",
            "callId",
            "tool",
            "request",
            "status",
            "decisionId",
            "outcome",
            "decidedBy",
            "authorizationDecisionId",
            "authorizationDecisionDigest",
            "reason",
            "decisionDigest",
            "resumeCommandId",
            "resumeCommandDigest",
            "aggregateVersion",
            "requestedAt",
            "expiresAt",
            "updatedAt",
            "decidedAt",
            "resumedAt",
            "cancelledAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "environmentId": uuid_schema(),
            "conversationId": uuid_schema(),
            "executionId": uuid_schema(),
            "id": uuid_schema(),
            "providerRunIdentityDigest": digest_schema(),
            "invocationProfileDigest": digest_schema(),
            "sourceEventSequence": nonnegative_sequence_schema(),
            "callId": bounded_utf8_line_schema(256),
            "tool": schema_ref("HarnessToolBinding"),
            "request": schema_ref("AgentToolPayloadIdentity"),
            "status": {
                "type": "string",
                "enum": ["pending", "approved", "denied", "expired", "resumed", "cancelled"]
            },
            "decisionId": nullable_uuid_schema(),
            "outcome": {
                "type": "string",
                "enum": ["approved", "denied", "expired"],
                "nullable": true
            },
            "decidedBy": nullable_uuid_schema(),
            "authorizationDecisionId": nullable_bounded_utf8_line_schema(512),
            "authorizationDecisionDigest": nullable_digest_schema(),
            "reason": nullable_bounded_utf8_line_schema(1024),
            "decisionDigest": nullable_digest_schema(),
            "resumeCommandId": nullable_uuid_schema(),
            "resumeCommandDigest": nullable_digest_schema(),
            "aggregateVersion": positive_sequence_schema(),
            "requestedAt": timestamp_schema(),
            "expiresAt": timestamp_schema(),
            "updatedAt": timestamp_schema(),
            "decidedAt": nullable_timestamp_schema(),
            "resumedAt": nullable_timestamp_schema(),
            "cancelledAt": nullable_timestamp_schema()
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
    bounded_array_schema(item, 200)
}

fn bounded_array_schema(item: &str, max_items: usize) -> Value {
    json!({
        "type": "array",
        "maxItems": max_items,
        "items": schema_ref(item)
    })
}

fn bounded_binding_array(item: &str) -> Value {
    let canonical_order = match item {
        "HarnessSkillBinding" | "HarnessMcpBinding" => &["assetId", "assetReleaseId"] as &[&str],
        "HarnessModelBinding" => &["modelId", "modelRevisionId"],
        "HarnessSecretReference" => &["name"],
        "HarnessToolBinding" => &["name", "revision"],
        _ => &[],
    };
    json!({
        "type": "array",
        "maxItems": 128,
        "uniqueItems": true,
        "x-a3s-canonical-order": canonical_order,
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

fn nullable_uuid_schema() -> Value {
    let mut schema = uuid_schema();
    schema["nullable"] = json!(true);
    schema
}

fn nullable_digest_schema() -> Value {
    let mut schema = digest_schema();
    schema["nullable"] = json!(true);
    schema
}

fn nullable_bounded_utf8_line_schema(max_bytes: usize) -> Value {
    let mut schema = bounded_utf8_line_schema(max_bytes);
    schema["nullable"] = json!(true);
    schema
}

fn bounded_utf8_line_schema(max_bytes: usize) -> Value {
    let mut schema = bounded_line_schema(max_bytes);
    schema["x-a3s-max-utf8-bytes"] = json!(max_bytes);
    schema["pattern"] = json!("^(?:\\S|\\S[^\\u0000\\r\\n]*\\S)$");
    schema
}

fn bounded_line_schema(max_length: usize) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": max_length,
        "pattern": "^[^\\u0000\\r\\n]+$"
    })
}
