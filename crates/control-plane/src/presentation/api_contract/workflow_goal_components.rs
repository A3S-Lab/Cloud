use super::workflow_components::{
    digest_schema, nullable_digest_schema, nullable_uuid_schema, timestamp_schema, uuid_schema,
};
use crate::modules::workflow::{
    WORKFLOW_GOAL_MAX_ACL_BYTES, WORKFLOW_GOAL_SCHEMA, WORKFLOW_PLAN_COMPILER_REVISION,
    WORKFLOW_PLAN_COMPILER_REVISION_V10, WORKFLOW_PLAN_COMPILER_REVISION_V11,
    WORKFLOW_PLAN_COMPILER_REVISION_V12, WORKFLOW_PLAN_COMPILER_REVISION_V2,
    WORKFLOW_PLAN_COMPILER_REVISION_V3, WORKFLOW_PLAN_COMPILER_REVISION_V4,
    WORKFLOW_PLAN_COMPILER_REVISION_V5, WORKFLOW_PLAN_COMPILER_REVISION_V6,
    WORKFLOW_PLAN_COMPILER_REVISION_V7, WORKFLOW_PLAN_COMPILER_REVISION_V8,
    WORKFLOW_PLAN_COMPILER_REVISION_V9, WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA,
    WORKFLOW_PLAN_SCHEMA_V10, WORKFLOW_PLAN_SCHEMA_V11, WORKFLOW_PLAN_SCHEMA_V12,
    WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_SCHEMA_V4,
    WORKFLOW_PLAN_SCHEMA_V5, WORKFLOW_PLAN_SCHEMA_V6, WORKFLOW_PLAN_SCHEMA_V7,
    WORKFLOW_PLAN_SCHEMA_V8, WORKFLOW_PLAN_SCHEMA_V9,
};
use a3s_cloud_contracts::{WORKFLOW_NODE_PROFILES_REVISION, WORKFLOW_NODE_PROFILES_SCHEMA};
use serde_json::{json, Map, Value};

pub(super) fn install_workflow_goal_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert("WorkflowStepKind".into(), workflow_step_kind_schema());
    schemas.insert("WorkflowDataType".into(), workflow_data_type_schema());
    schemas.insert(
        "WorkflowNodeCatalogEntry".into(),
        workflow_node_catalog_entry_schema(),
    );
    schemas.insert("WorkflowNodeCatalog".into(), workflow_node_catalog_schema());
    schemas.insert(
        "WorkflowCapabilityReference".into(),
        workflow_capability_reference_schema(),
    );
    schemas.insert("WorkflowStepPort".into(), workflow_step_port_schema());
    schemas.insert(
        "WorkflowStepFailureContract".into(),
        workflow_step_failure_contract_schema(),
    );
    schemas.insert(
        "WorkflowStepDefaultOutputContract".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["outputPort"],
            "properties": {
                "outputPort": { "$ref": "#/components/schemas/WorkflowStepPort" }
            }
        }),
    );
    schemas.insert(
        "WorkflowStepDescriptorBinding".into(),
        workflow_step_descriptor_binding_schema(),
    );
    schemas.insert("WorkflowPlanStep".into(), workflow_plan_step_schema());
    schemas.insert("WorkflowPlanEdge".into(), workflow_plan_edge_schema());
    schemas.insert("WorkflowPlan".into(), workflow_plan_schema());
    schemas.insert(
        "WorkflowPlanRevision".into(),
        workflow_plan_revision_schema(),
    );
    schemas.insert("WorkflowGoal".into(), workflow_goal_schema());
    schemas.insert(
        "WorkflowGoalList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/WorkflowGoal" }
        }),
    );
    schemas.insert(
        "WorkflowGoalMutation".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["goal", "planRevision", "replayed"],
            "properties": {
                "goal": { "$ref": "#/components/schemas/WorkflowGoal" },
                "planRevision": { "$ref": "#/components/schemas/WorkflowPlanRevision" },
                "replayed": { "type": "boolean" }
            }
        }),
    );
}

fn workflow_node_catalog_entry_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "capabilityId", "label", "owner", "gate", "gateState", "dependencies",
            "availability", "kind", "executionClass", "semanticProfiles", "evidence",
            "unavailableReason"
        ],
        "properties": {
            "capabilityId": { "type": "string", "minLength": 1 },
            "label": { "type": "string", "minLength": 1 },
            "owner": {
                "type": "string",
                "enum": [
                    "agents", "applications", "assets", "automations", "connectors",
                    "edge_gateway", "executions", "files", "identity", "inference",
                    "knowledge", "operations_telemetry", "platform", "use", "workflow"
                ]
            },
            "gate": { "type": "string", "minLength": 1 },
            "gateState": {
                "type": "string",
                "enum": ["planned", "in_progress", "implemented", "verified"]
            },
            "dependencies": {
                "type": "array",
                "items": { "type": "string", "minLength": 1 }
            },
            "availability": {
                "type": "string",
                "enum": ["unavailable", "internal", "public"]
            },
            "kind": nullable_workflow_step_kind_schema(),
            "executionClass": {
                "type": "string",
                "enum": [
                    "workflow_local", "composite_region", "owning_application_port",
                    "invocation_only"
                ]
            },
            "semanticProfiles": {
                "type": "array",
                "items": { "type": "string", "minLength": 1 }
            },
            "evidence": {
                "type": "array",
                "items": { "type": "string", "minLength": 1 }
            },
            "unavailableReason": nullable_string_schema()
        }
    })
}

fn workflow_node_catalog_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "revision", "baseline", "parityManifestDigest", "profileSetDigest",
            "parityClaim", "nodes"
        ],
        "properties": {
            "schema": { "type": "string", "enum": [WORKFLOW_NODE_PROFILES_SCHEMA] },
            "revision": { "type": "string", "enum": [WORKFLOW_NODE_PROFILES_REVISION] },
            "baseline": { "type": "string", "minLength": 1 },
            "parityManifestDigest": digest_schema(),
            "profileSetDigest": digest_schema(),
            "parityClaim": { "type": "boolean" },
            "nodes": {
                "type": "array",
                "minItems": 23,
                "maxItems": 23,
                "items": { "$ref": "#/components/schemas/WorkflowNodeCatalogEntry" }
            }
        }
    })
}

fn workflow_capability_reference_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["owner", "type", "resourceId", "revision", "digest", "capability"],
        "properties": {
            "owner": {
                "type": "string",
                "enum": ["assets", "workflow", "inference", "use", "executions"]
            },
            "type": {
                "type": "string",
                "enum": [
                    "agent_release", "mcp_service_profile", "workflow_revision",
                    "model_revision", "use_package", "execution_template", "connector_revision"
                ]
            },
            "resourceId": uuid_schema(),
            "revision": { "type": "string", "minLength": 1 },
            "digest": digest_schema(),
            "capability": { "type": "string", "minLength": 1 }
        }
    })
}

fn workflow_step_port_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "valueType", "cardinality", "required", "dynamic"],
        "properties": {
            "name": { "type": "string", "minLength": 1, "maxLength": 128 },
            "valueType": workflow_data_type_schema(),
            "cardinality": { "type": "string", "enum": ["single", "many"] },
            "required": { "type": "boolean" },
            "dynamic": { "type": "boolean" }
        }
    })
}

fn workflow_step_failure_contract_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["errorOutput", "retryClassification", "fallback", "failureBranch"],
        "properties": {
            "errorOutput": nullable_ref("#/components/schemas/WorkflowStepPort"),
            "retryClassification": {
                "type": "string",
                "enum": ["not_retryable", "flow_retryable", "owner_classified"]
            },
            "fallback": {
                "type": "string",
                "enum": ["unsupported", "default_output", "failure_branch"]
            },
            "failureBranch": { "type": "boolean" }
        }
    })
}

fn workflow_step_descriptor_binding_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["stepId", "descriptorId", "descriptorRevision", "semanticDigest"],
        "properties": {
            "stepId": identifier_schema(),
            "descriptorId": { "type": "string", "minLength": 1 },
            "descriptorRevision": { "type": "string", "minLength": 1 },
            "semanticDigest": digest_schema()
        }
    })
}

fn workflow_plan_step_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "id", "kind", "configurationDigest", "inputSchemaDigest", "outputSchemaDigest",
            "policyDigest", "capability", "descriptor"
        ],
        "properties": {
            "id": identifier_schema(),
            "kind": workflow_step_kind_schema(),
            "configurationDigest": digest_schema(),
            "inputSchemaDigest": digest_schema(),
            "outputSchemaDigest": digest_schema(),
            "policyDigest": nullable_digest_schema(),
            "capability": nullable_ref("#/components/schemas/WorkflowCapabilityReference"),
            "descriptor": nullable_ref("#/components/schemas/WorkflowStepDescriptorBinding"),
            "failure": { "$ref": "#/components/schemas/WorkflowStepFailureContract" },
            "defaultOutput": {
                "$ref": "#/components/schemas/WorkflowStepDefaultOutputContract"
            }
        }
    })
}

fn workflow_plan_edge_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "source", "target", "sourceHandle"],
        "properties": {
            "id": identifier_schema(),
            "source": identifier_schema(),
            "target": identifier_schema(),
            "sourceHandle": nullable_string_schema()
        }
    })
}

fn workflow_plan_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "compilerRevision", "workflowDefinitionId", "workflowRevisionId",
            "workflowDigest", "workflowPayloadSetDigest", "semanticContractSetDigest",
            "variableContractDigest", "compositeRegionsDigest", "ontologyId",
            "ontologyRevisionId", "ontologyDigest", "environmentId", "inputDigest", "steps",
            "edges"
        ],
        "properties": {
            "schema": workflow_plan_schema_version(),
            "compilerRevision": workflow_plan_compiler_revision_schema(),
            "workflowDefinitionId": uuid_schema(),
            "workflowRevisionId": uuid_schema(),
            "workflowDigest": digest_schema(),
            "workflowPayloadSetDigest": digest_schema(),
            "semanticContractSetDigest": nullable_digest_schema(),
            "variableContractDigest": nullable_digest_schema(),
            "compositeRegionsDigest": nullable_digest_schema(),
            "ontologyId": uuid_schema(),
            "ontologyRevisionId": uuid_schema(),
            "ontologyDigest": digest_schema(),
            "environmentId": nullable_uuid_schema(),
            "inputDigest": digest_schema(),
            "steps": {
                "type": "array",
                "minItems": 1,
                "maxItems": 10_000,
                "items": { "$ref": "#/components/schemas/WorkflowPlanStep" }
            },
            "edges": {
                "type": "array",
                "maxItems": 100_000,
                "items": { "$ref": "#/components/schemas/WorkflowPlanEdge" }
            }
        }
    })
}

fn workflow_plan_revision_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "workflowGoalId", "id", "schema",
            "compilerRevision", "digest", "canonicalPlan", "plan", "createdBy", "createdAt"
        ],
        "properties": {
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "workflowGoalId": uuid_schema(),
            "id": uuid_schema(),
            "schema": workflow_plan_schema_version(),
            "compilerRevision": workflow_plan_compiler_revision_schema(),
            "digest": digest_schema(),
            "canonicalPlan": {
                "type": "string",
                "minLength": 1,
                "maxLength": WORKFLOW_PLAN_MAX_BYTES
            },
            "plan": { "$ref": "#/components/schemas/WorkflowPlan" },
            "createdBy": uuid_schema(),
            "createdAt": timestamp_schema()
        }
    })
}

fn workflow_goal_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "id", "name", "contractSchema", "contractDigest",
            "inputDigest", "canonicalGoalAcl", "workflowDefinitionId", "workflowRevisionId",
            "workflowDigest", "ontologyId", "ontologyRevisionId", "ontologyDigest",
            "environmentId", "input", "planRevisionId", "planDigest", "createdBy", "createdAt"
        ],
        "properties": {
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "id": uuid_schema(),
            "name": { "type": "string", "minLength": 1, "maxLength": 120 },
            "contractSchema": { "type": "string", "enum": [WORKFLOW_GOAL_SCHEMA] },
            "contractDigest": digest_schema(),
            "inputDigest": digest_schema(),
            "canonicalGoalAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": WORKFLOW_GOAL_MAX_ACL_BYTES
            },
            "workflowDefinitionId": uuid_schema(),
            "workflowRevisionId": uuid_schema(),
            "workflowDigest": digest_schema(),
            "ontologyId": uuid_schema(),
            "ontologyRevisionId": uuid_schema(),
            "ontologyDigest": digest_schema(),
            "environmentId": nullable_uuid_schema(),
            "input": {},
            "planRevisionId": uuid_schema(),
            "planDigest": digest_schema(),
            "createdBy": uuid_schema(),
            "createdAt": timestamp_schema()
        }
    })
}

fn workflow_plan_schema_version() -> Value {
    json!({
        "type": "string",
        "enum": [
            WORKFLOW_PLAN_SCHEMA, WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_PLAN_SCHEMA_V3,
            WORKFLOW_PLAN_SCHEMA_V4, WORKFLOW_PLAN_SCHEMA_V5, WORKFLOW_PLAN_SCHEMA_V6,
            WORKFLOW_PLAN_SCHEMA_V7, WORKFLOW_PLAN_SCHEMA_V8, WORKFLOW_PLAN_SCHEMA_V9,
            WORKFLOW_PLAN_SCHEMA_V10, WORKFLOW_PLAN_SCHEMA_V11, WORKFLOW_PLAN_SCHEMA_V12
        ]
    })
}

fn workflow_plan_compiler_revision_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_COMPILER_REVISION_V2,
            WORKFLOW_PLAN_COMPILER_REVISION_V3, WORKFLOW_PLAN_COMPILER_REVISION_V4,
            WORKFLOW_PLAN_COMPILER_REVISION_V5, WORKFLOW_PLAN_COMPILER_REVISION_V6,
            WORKFLOW_PLAN_COMPILER_REVISION_V7, WORKFLOW_PLAN_COMPILER_REVISION_V8,
            WORKFLOW_PLAN_COMPILER_REVISION_V9, WORKFLOW_PLAN_COMPILER_REVISION_V10,
            WORKFLOW_PLAN_COMPILER_REVISION_V11, WORKFLOW_PLAN_COMPILER_REVISION_V12
        ]
    })
}

fn workflow_step_kind_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            "input", "output", "transform", "branch", "human_decision", "execution",
            "agent", "mcp", "model", "tool", "service", "memory", "subworkflow"
        ]
    })
}

fn nullable_workflow_step_kind_schema() -> Value {
    let mut schema = workflow_step_kind_schema();
    schema["nullable"] = json!(true);
    schema
}

fn workflow_data_type_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["any", "object", "array", "string", "number", "boolean", "null"]
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

fn nullable_string_schema() -> Value {
    json!({ "type": "string", "nullable": true })
}

fn nullable_ref(reference: &str) -> Value {
    json!({
        "allOf": [{ "$ref": reference }],
        "nullable": true
    })
}
