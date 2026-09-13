use crate::modules::knowledge::{
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
    KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1, MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
use serde_json::{Map, Value, json};

pub(super) const KNOWLEDGE_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    ("KnowledgeBaseSuccessResponse", "KnowledgeBase"),
    ("KnowledgeBaseListSuccessResponse", "KnowledgeBaseList"),
    (
        "KnowledgeBaseMutationSuccessResponse",
        "KnowledgeBaseMutation",
    ),
    ("KnowledgePipelineSuccessResponse", "KnowledgePipeline"),
    (
        "KnowledgePipelineListSuccessResponse",
        "KnowledgePipelineList",
    ),
    (
        "KnowledgePipelineMutationSuccessResponse",
        "KnowledgePipelineMutation",
    ),
];

pub(super) const KNOWLEDGE_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "KnowledgeBaseSuccess200",
        200,
        "KnowledgeBaseSuccessResponse",
    ),
    (
        "KnowledgeBaseListSuccess200",
        200,
        "KnowledgeBaseListSuccessResponse",
    ),
    (
        "KnowledgeBaseMutationSuccess200",
        200,
        "KnowledgeBaseMutationSuccessResponse",
    ),
    (
        "KnowledgeBaseMutationSuccess201",
        201,
        "KnowledgeBaseMutationSuccessResponse",
    ),
    (
        "KnowledgePipelineSuccess200",
        200,
        "KnowledgePipelineSuccessResponse",
    ),
    (
        "KnowledgePipelineListSuccess200",
        200,
        "KnowledgePipelineListSuccessResponse",
    ),
    (
        "KnowledgePipelineMutationSuccess200",
        200,
        "KnowledgePipelineMutationSuccessResponse",
    ),
    (
        "KnowledgePipelineMutationSuccess201",
        201,
        "KnowledgePipelineMutationSuccessResponse",
    ),
];

pub(super) fn install_knowledge_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("KnowledgeBase", knowledge_base_schema()),
        ("KnowledgeBaseList", knowledge_base_list_schema()),
        ("KnowledgeBaseMutation", knowledge_base_mutation_schema()),
        ("KnowledgePipeline", knowledge_pipeline_schema()),
        ("KnowledgePipelineList", knowledge_pipeline_list_schema()),
        (
            "KnowledgePipelineMutation",
            knowledge_pipeline_mutation_schema(),
        ),
    ] {
        schemas.insert(name.into(), schema);
    }
}

fn knowledge_base_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "knowledgeBaseId",
            "revisionId",
            "generation",
            "name",
            "contractSchema",
            "revisionAcl",
            "revisionDigest",
            "createdAt",
            "updatedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "knowledgeBaseId": uuid_schema(),
            "revisionId": uuid_schema(),
            "generation": { "type": "integer", "minimum": 1 },
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_BASE_REVISION_SCHEMA_V1]
            },
            "revisionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgeBase revision contract."
            },
            "revisionDigest": digest_schema(),
            "createdAt": timestamp_schema(),
            "updatedAt": timestamp_schema()
        }),
    )
}

fn knowledge_base_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
        "items": schema_ref("KnowledgeBase")
    })
}

fn knowledge_base_mutation_schema() -> Value {
    object_schema(
        &["knowledgeBase", "replayed"],
        json!({
            "knowledgeBase": schema_ref("KnowledgeBase"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn knowledge_pipeline_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "pipelineId",
            "releaseId",
            "name",
            "contractSchema",
            "releaseAcl",
            "releaseDigest",
            "createdAt",
            "updatedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "pipelineId": uuid_schema(),
            "releaseId": uuid_schema(),
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1]
            },
            "releaseAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgePipeline release contract."
            },
            "releaseDigest": digest_schema(),
            "createdAt": timestamp_schema(),
            "updatedAt": timestamp_schema()
        }),
    )
}

fn knowledge_pipeline_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
        "items": schema_ref("KnowledgePipeline")
    })
}

fn knowledge_pipeline_mutation_schema() -> Value {
    object_schema(
        &["knowledgePipeline", "replayed"],
        json!({
            "knowledgePipeline": schema_ref("KnowledgePipeline"),
            "replayed": { "type": "boolean" }
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

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn uuid_schema() -> Value {
    json!({ "type": "string", "format": "uuid" })
}

fn digest_schema() -> Value {
    json!({ "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" })
}

fn timestamp_schema() -> Value {
    json!({ "type": "string", "format": "date-time" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_schemas_are_closed_and_acl_first() {
        let mut schemas = Map::new();
        install_knowledge_component_schemas(&mut schemas);

        for name in [
            "KnowledgeBase",
            "KnowledgeBaseMutation",
            "KnowledgePipeline",
            "KnowledgePipelineMutation",
        ] {
            assert_eq!(schemas[name]["additionalProperties"], false, "{name}");
        }
        assert_eq!(
            schemas["KnowledgeBase"]["properties"]["revisionAcl"]["maxLength"],
            KNOWLEDGE_CONTRACT_MAX_ACL_BYTES
        );
        assert_eq!(
            schemas["KnowledgeBaseList"]["maxItems"],
            MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT
        );
        assert_eq!(
            schemas["KnowledgePipelineList"]["maxItems"],
            MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT
        );
    }
}
