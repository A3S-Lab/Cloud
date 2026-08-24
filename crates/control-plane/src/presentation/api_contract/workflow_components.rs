use crate::modules::workflow::{
    WORKFLOW_COMPILER_SCHEMA_VERSION, WORKFLOW_COMPILER_SCHEMA_VERSION_V2,
    WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES, WORKFLOW_COMPOSITE_REGIONS_SCHEMA,
    WORKFLOW_CONFIGURATION_SCHEMA, WORKFLOW_DATA_SCHEMA, WORKFLOW_DEFINITION_SCHEMA,
    WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA, WORKFLOW_PAYLOAD_MAX_ACL_BYTES,
    WORKFLOW_POLICY_SCHEMA, WORKFLOW_POLICY_SCHEMA_V2, WORKFLOW_POLICY_SCHEMA_V3,
    WORKFLOW_REVISION_MAX_PAYLOADS, WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES,
    WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA, WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES,
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA, WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA,
    WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES, WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
    WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES, WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
};
use serde_json::{json, Map, Value};

const WORKFLOW_DEFINITION_MAX_ACL_BYTES: usize = 1024 * 1024;
const MAXIMUM_JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) fn install_workflow_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert(
        "WorkflowPayloadSchema".into(),
        json!({
            "type": "string",
            "enum": [
                WORKFLOW_CONFIGURATION_SCHEMA,
                WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA,
                WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA,
                WORKFLOW_DATA_SCHEMA,
                WORKFLOW_POLICY_SCHEMA,
                WORKFLOW_POLICY_SCHEMA_V2,
                WORKFLOW_POLICY_SCHEMA_V3
            ]
        }),
    );
    schemas.insert(
        "WorkflowConfigurationPayload".into(),
        workflow_payload_variant_schema(
            "configuration",
            &[
                WORKFLOW_CONFIGURATION_SCHEMA,
                WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA,
                WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA,
            ],
        ),
    );
    schemas.insert(
        "WorkflowDataSchemaPayload".into(),
        workflow_payload_variant_schema("data_schema", &[WORKFLOW_DATA_SCHEMA]),
    );
    schemas.insert(
        "WorkflowPolicyPayload".into(),
        workflow_payload_variant_schema(
            "policy",
            &[
                WORKFLOW_POLICY_SCHEMA,
                WORKFLOW_POLICY_SCHEMA_V2,
                WORKFLOW_POLICY_SCHEMA_V3,
            ],
        ),
    );
    schemas.insert(
        "WorkflowPayload".into(),
        json!({
            "oneOf": [
                { "$ref": "#/components/schemas/WorkflowConfigurationPayload" },
                { "$ref": "#/components/schemas/WorkflowDataSchemaPayload" },
                { "$ref": "#/components/schemas/WorkflowPolicyPayload" }
            ],
            "discriminator": {
                "propertyName": "kind",
                "mapping": {
                    "configuration": "#/components/schemas/WorkflowConfigurationPayload",
                    "data_schema": "#/components/schemas/WorkflowDataSchemaPayload",
                    "policy": "#/components/schemas/WorkflowPolicyPayload"
                }
            }
        }),
    );

    schemas.insert(
        "WorkflowSemanticContractSchema".into(),
        json!({
            "type": "string",
            "enum": [
                WORKFLOW_COMPOSITE_REGIONS_SCHEMA,
                WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
                WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
                WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
                WORKFLOW_VARIABLE_DEFAULTS_SCHEMA
            ]
        }),
    );
    install_semantic_contract_variants(schemas);
    schemas.insert(
        "WorkflowSemanticContract".into(),
        json!({
            "oneOf": [
                { "$ref": "#/components/schemas/WorkflowCompositeRegionsSemanticContract" },
                { "$ref": "#/components/schemas/WorkflowDescriptorBindingsSemanticContract" },
                { "$ref": "#/components/schemas/WorkflowDescriptorRegistrySemanticContract" },
                { "$ref": "#/components/schemas/WorkflowVariableContractSemanticContract" },
                { "$ref": "#/components/schemas/WorkflowVariableDefaultsSemanticContract" }
            ],
            "discriminator": {
                "propertyName": "kind",
                "mapping": {
                    "composite_regions": "#/components/schemas/WorkflowCompositeRegionsSemanticContract",
                    "descriptor_bindings": "#/components/schemas/WorkflowDescriptorBindingsSemanticContract",
                    "descriptor_registry": "#/components/schemas/WorkflowDescriptorRegistrySemanticContract",
                    "variable_contract": "#/components/schemas/WorkflowVariableContractSemanticContract",
                    "variable_defaults": "#/components/schemas/WorkflowVariableDefaultsSemanticContract"
                }
            }
        }),
    );

    schemas.insert("WorkflowDefinition".into(), workflow_definition_schema());
    schemas.insert(
        "WorkflowDefinitionList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/WorkflowDefinition" }
        }),
    );
    schemas.insert(
        "WorkflowRevisionSummary".into(),
        workflow_revision_summary_schema(),
    );
    schemas.insert(
        "WorkflowRevisionSummaryList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/WorkflowRevisionSummary" }
        }),
    );
    schemas.insert("WorkflowRevision".into(), workflow_revision_schema());
    schemas.insert(
        "WorkflowDefinitionMutation".into(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["workflowDefinition", "revision", "replayed"],
            "properties": {
                "workflowDefinition": { "$ref": "#/components/schemas/WorkflowDefinition" },
                "revision": { "$ref": "#/components/schemas/WorkflowRevision" },
                "replayed": { "type": "boolean" }
            }
        }),
    );
}

fn workflow_payload_variant_schema(kind: &str, supported_schemas: &[&str]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "schema", "digest", "canonicalAcl"],
        "properties": {
            "kind": { "type": "string", "enum": [kind] },
            "schema": {
                "allOf": [
                    { "$ref": "#/components/schemas/WorkflowPayloadSchema" },
                    { "type": "string", "enum": supported_schemas }
                ]
            },
            "digest": digest_schema(),
            "canonicalAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": WORKFLOW_PAYLOAD_MAX_ACL_BYTES
            }
        }
    })
}

fn install_semantic_contract_variants(schemas: &mut Map<String, Value>) {
    for (name, kind, schema, max_acl_bytes) in [
        (
            "WorkflowCompositeRegionsSemanticContract",
            "composite_regions",
            WORKFLOW_COMPOSITE_REGIONS_SCHEMA,
            WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES,
        ),
        (
            "WorkflowDescriptorBindingsSemanticContract",
            "descriptor_bindings",
            WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
            WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES,
        ),
        (
            "WorkflowDescriptorRegistrySemanticContract",
            "descriptor_registry",
            WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
            WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES,
        ),
        (
            "WorkflowVariableContractSemanticContract",
            "variable_contract",
            WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
            WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES,
        ),
        (
            "WorkflowVariableDefaultsSemanticContract",
            "variable_defaults",
            WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
            WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES,
        ),
    ] {
        schemas.insert(
            name.into(),
            workflow_semantic_contract_variant_schema(kind, schema, max_acl_bytes),
        );
    }
}

fn workflow_semantic_contract_variant_schema(
    kind: &str,
    schema: &str,
    max_acl_bytes: usize,
) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "schema", "digest", "canonicalAcl"],
        "properties": {
            "kind": { "type": "string", "enum": [kind] },
            "schema": {
                "allOf": [
                    { "$ref": "#/components/schemas/WorkflowSemanticContractSchema" },
                    { "type": "string", "enum": [schema] }
                ]
            },
            "digest": digest_schema(),
            "canonicalAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": max_acl_bytes
            }
        }
    })
}

fn workflow_definition_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "id", "name", "description",
            "currentRevisionId", "currentRevisionNumber", "currentRevisionDigest",
            "aggregateVersion", "createdBy", "createdAt", "updatedAt"
        ],
        "properties": {
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "id": uuid_schema(),
            "name": { "type": "string", "minLength": 1, "maxLength": 120 },
            "description": { "type": "string", "maxLength": 4096 },
            "currentRevisionId": uuid_schema(),
            "currentRevisionNumber": revision_number_schema(),
            "currentRevisionDigest": digest_schema(),
            "aggregateVersion": revision_number_schema(),
            "createdBy": uuid_schema(),
            "createdAt": timestamp_schema(),
            "updatedAt": timestamp_schema()
        }
    })
}

fn workflow_revision_summary_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "workflowDefinitionId", "id",
            "revisionNumber", "parentRevisionId", "parentDigest", "contractSchema",
            "compilerSchemaVersion", "contentDigest", "payloadSetDigest", "payloadCount",
            "semanticContractSetDigest", "semanticContractCount", "createdBy", "createdAt"
        ],
        "properties": workflow_revision_summary_properties()
    })
}

fn workflow_revision_schema() -> Value {
    let mut properties = workflow_revision_summary_properties();
    properties.insert(
        "canonicalDefinitionAcl".into(),
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": WORKFLOW_DEFINITION_MAX_ACL_BYTES
        }),
    );
    properties.insert(
        "payloads".into(),
        json!({
            "type": "array",
            "minItems": 1,
            "maxItems": WORKFLOW_REVISION_MAX_PAYLOADS,
            "items": { "$ref": "#/components/schemas/WorkflowPayload" }
        }),
    );
    properties.insert(
        "semanticContracts".into(),
        json!({
            "type": "array",
            "maxItems": 5,
            "items": { "$ref": "#/components/schemas/WorkflowSemanticContract" }
        }),
    );
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "organizationId", "projectId", "workflowDefinitionId", "id",
            "revisionNumber", "parentRevisionId", "parentDigest", "contractSchema",
            "compilerSchemaVersion", "contentDigest", "payloadSetDigest", "payloadCount",
            "semanticContractSetDigest", "semanticContractCount", "createdBy", "createdAt",
            "canonicalDefinitionAcl", "payloads", "semanticContracts"
        ],
        "properties": properties
    })
}

fn workflow_revision_summary_properties() -> Map<String, Value> {
    [
        ("organizationId", uuid_schema()),
        ("projectId", uuid_schema()),
        ("workflowDefinitionId", uuid_schema()),
        ("id", uuid_schema()),
        ("revisionNumber", revision_number_schema()),
        ("parentRevisionId", nullable_uuid_schema()),
        ("parentDigest", nullable_digest_schema()),
        (
            "contractSchema",
            json!({ "type": "string", "enum": [WORKFLOW_DEFINITION_SCHEMA] }),
        ),
        (
            "compilerSchemaVersion",
            json!({
                "type": "integer",
                "enum": [
                    WORKFLOW_COMPILER_SCHEMA_VERSION,
                    WORKFLOW_COMPILER_SCHEMA_VERSION_V2
                ]
            }),
        ),
        ("contentDigest", digest_schema()),
        ("payloadSetDigest", digest_schema()),
        (
            "payloadCount",
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": WORKFLOW_REVISION_MAX_PAYLOADS
            }),
        ),
        ("semanticContractSetDigest", nullable_digest_schema()),
        (
            "semanticContractCount",
            json!({ "type": "integer", "minimum": 0, "maximum": 5 }),
        ),
        ("createdBy", uuid_schema()),
        ("createdAt", timestamp_schema()),
    ]
    .into_iter()
    .map(|(name, schema)| (name.to_owned(), schema))
    .collect()
}

fn uuid_schema() -> Value {
    json!({ "type": "string", "format": "uuid" })
}

fn nullable_uuid_schema() -> Value {
    json!({ "type": "string", "format": "uuid", "nullable": true })
}

fn timestamp_schema() -> Value {
    json!({ "type": "string", "format": "date-time" })
}

fn revision_number_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

fn digest_schema() -> Value {
    json!({ "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" })
}

fn nullable_digest_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "^sha256:[0-9a-f]{64}$",
        "nullable": true
    })
}
