use crate::modules::knowledge::{
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_REVISION_SCHEMA_V1,
    KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DOCUMENT_SCHEMA_V1,
    KNOWLEDGE_INDEX_REVISION_SCHEMA_V1, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1, MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT, MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
use serde_json::{json, Map, Value};

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
    ("KnowledgeDocumentSuccessResponse", "KnowledgeDocument"),
    ("KnowledgeDocumentListSuccessResponse", "KnowledgeDocumentList"),
    (
        "KnowledgeDocumentMutationSuccessResponse",
        "KnowledgeDocumentMutation",
    ),
    ("KnowledgeChunkSuccessResponse", "KnowledgeChunk"),
    ("KnowledgeChunkListSuccessResponse", "KnowledgeChunkList"),
    (
        "KnowledgeChunkMutationSuccessResponse",
        "KnowledgeChunkMutation",
    ),
    (
        "KnowledgeIndexRevisionSuccessResponse",
        "KnowledgeIndexRevision",
    ),
    (
        "KnowledgeIndexRevisionMutationSuccessResponse",
        "KnowledgeIndexRevisionMutation",
    ),
    (
        "KnowledgeRetrievalPolicyRevisionSuccessResponse",
        "KnowledgeRetrievalPolicyRevision",
    ),
    (
        "KnowledgeRetrievalPolicyRevisionMutationSuccessResponse",
        "KnowledgeRetrievalPolicyRevisionMutation",
    ),
    (
        "ExternalKnowledgeBindingSuccessResponse",
        "ExternalKnowledgeBinding",
    ),
    (
        "ExternalKnowledgeBindingMutationSuccessResponse",
        "ExternalKnowledgeBindingMutation",
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
    (
        "KnowledgeDocumentSuccess200",
        200,
        "KnowledgeDocumentSuccessResponse",
    ),
    (
        "KnowledgeDocumentListSuccess200",
        200,
        "KnowledgeDocumentListSuccessResponse",
    ),
    (
        "KnowledgeDocumentMutationSuccess200",
        200,
        "KnowledgeDocumentMutationSuccessResponse",
    ),
    (
        "KnowledgeDocumentMutationSuccess201",
        201,
        "KnowledgeDocumentMutationSuccessResponse",
    ),
    (
        "KnowledgeChunkSuccess200",
        200,
        "KnowledgeChunkSuccessResponse",
    ),
    (
        "KnowledgeChunkListSuccess200",
        200,
        "KnowledgeChunkListSuccessResponse",
    ),
    (
        "KnowledgeChunkMutationSuccess200",
        200,
        "KnowledgeChunkMutationSuccessResponse",
    ),
    (
        "KnowledgeChunkMutationSuccess201",
        201,
        "KnowledgeChunkMutationSuccessResponse",
    ),
    (
        "KnowledgeIndexRevisionSuccess200",
        200,
        "KnowledgeIndexRevisionSuccessResponse",
    ),
    (
        "KnowledgeIndexRevisionMutationSuccess200",
        200,
        "KnowledgeIndexRevisionMutationSuccessResponse",
    ),
    (
        "KnowledgeIndexRevisionMutationSuccess201",
        201,
        "KnowledgeIndexRevisionMutationSuccessResponse",
    ),
    (
        "KnowledgeRetrievalPolicyRevisionSuccess200",
        200,
        "KnowledgeRetrievalPolicyRevisionSuccessResponse",
    ),
    (
        "KnowledgeRetrievalPolicyRevisionMutationSuccess200",
        200,
        "KnowledgeRetrievalPolicyRevisionMutationSuccessResponse",
    ),
    (
        "KnowledgeRetrievalPolicyRevisionMutationSuccess201",
        201,
        "KnowledgeRetrievalPolicyRevisionMutationSuccessResponse",
    ),
    (
        "ExternalKnowledgeBindingSuccess200",
        200,
        "ExternalKnowledgeBindingSuccessResponse",
    ),
    (
        "ExternalKnowledgeBindingMutationSuccess200",
        200,
        "ExternalKnowledgeBindingMutationSuccessResponse",
    ),
    (
        "ExternalKnowledgeBindingMutationSuccess201",
        201,
        "ExternalKnowledgeBindingMutationSuccessResponse",
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
        ("KnowledgeDocument", knowledge_document_schema()),
        ("KnowledgeDocumentList", knowledge_document_list_schema()),
        (
            "KnowledgeDocumentMutation",
            knowledge_document_mutation_schema(),
        ),
        ("KnowledgeChunk", knowledge_chunk_schema()),
        ("KnowledgeChunkList", knowledge_chunk_list_schema()),
        ("KnowledgeChunkMutation", knowledge_chunk_mutation_schema()),
        ("KnowledgeIndexRevision", knowledge_index_revision_schema()),
        (
            "KnowledgeIndexRevisionMutation",
            knowledge_index_revision_mutation_schema(),
        ),
        (
            "KnowledgeRetrievalPolicyRevision",
            knowledge_retrieval_policy_revision_schema(),
        ),
        (
            "KnowledgeRetrievalPolicyRevisionMutation",
            knowledge_retrieval_policy_revision_mutation_schema(),
        ),
        ("ExternalKnowledgeBinding", external_knowledge_binding_schema()),
        (
            "ExternalKnowledgeBindingMutation",
            external_knowledge_binding_mutation_schema(),
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

fn knowledge_document_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "documentId",
            "knowledgeBaseId",
            "knowledgeBaseRevisionId",
            "title",
            "contractSchema",
            "documentAcl",
            "documentDigest",
            "createdAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "documentId": uuid_schema(),
            "knowledgeBaseId": uuid_schema(),
            "knowledgeBaseRevisionId": uuid_schema(),
            "title": { "type": "string", "minLength": 1, "maxLength": 63 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_DOCUMENT_SCHEMA_V1]
            },
            "documentAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgeDocument contract."
            },
            "documentDigest": digest_schema(),
            "createdAt": timestamp_schema()
        }),
    )
}

fn knowledge_document_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT,
        "items": schema_ref("KnowledgeDocument")
    })
}

fn knowledge_chunk_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT,
        "items": schema_ref("KnowledgeChunk")
    })
}

fn knowledge_document_mutation_schema() -> Value {
    object_schema(
        &["knowledgeDocument", "replayed"],
        json!({
            "knowledgeDocument": schema_ref("KnowledgeDocument"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn knowledge_chunk_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "documentId",
            "chunkId",
            "ordinal",
            "contractSchema",
            "chunkAcl",
            "chunkDigest",
            "createdAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "documentId": uuid_schema(),
            "chunkId": uuid_schema(),
            "ordinal": { "type": "integer", "minimum": 0 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_CHUNK_SCHEMA_V1]
            },
            "chunkAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgeChunk contract."
            },
            "chunkDigest": digest_schema(),
            "createdAt": timestamp_schema()
        }),
    )
}

fn knowledge_chunk_mutation_schema() -> Value {
    object_schema(
        &["knowledgeChunk", "replayed"],
        json!({
            "knowledgeChunk": schema_ref("KnowledgeChunk"),
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


fn knowledge_index_revision_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "knowledgeBaseRevisionId",
            "indexRevisionId",
            "strategy",
            "embeddingDimension",
            "contractSchema",
            "indexAcl",
            "indexDigest",
            "createdAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "knowledgeBaseRevisionId": uuid_schema(),
            "indexRevisionId": uuid_schema(),
            "strategy": {
                "type": "string",
                "enum": ["vector", "full_text", "hybrid", "inverted"]
            },
            "embeddingDimension": { "type": "integer", "minimum": 0 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_INDEX_REVISION_SCHEMA_V1]
            },
            "indexAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgeIndexRevision contract."
            },
            "indexDigest": digest_schema(),
            "createdAt": timestamp_schema()
        }),
    )
}

fn knowledge_index_revision_mutation_schema() -> Value {
    object_schema(
        &["knowledgeIndexRevision", "replayed"],
        json!({
            "knowledgeIndexRevision": schema_ref("KnowledgeIndexRevision"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn knowledge_retrieval_policy_revision_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "knowledgeBaseRevisionId",
            "policyRevisionId",
            "searchMode",
            "topK",
            "contractSchema",
            "policyAcl",
            "policyDigest",
            "createdAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "knowledgeBaseRevisionId": uuid_schema(),
            "policyRevisionId": uuid_schema(),
            "searchMode": { "type": "string", "minLength": 1 },
            "topK": { "type": "integer", "minimum": 1 },
            "contractSchema": {
                "type": "string",
                "enum": [KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1]
            },
            "policyAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL KnowledgeRetrievalPolicyRevision contract."
            },
            "policyDigest": digest_schema(),
            "createdAt": timestamp_schema()
        }),
    )
}

fn knowledge_retrieval_policy_revision_mutation_schema() -> Value {
    object_schema(
        &["knowledgeRetrievalPolicyRevision", "replayed"],
        json!({
            "knowledgeRetrievalPolicyRevision": schema_ref("KnowledgeRetrievalPolicyRevision"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn external_knowledge_binding_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "knowledgeBaseId",
            "bindingId",
            "displayName",
            "contractSchema",
            "bindingAcl",
            "bindingDigest",
            "createdAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "knowledgeBaseId": uuid_schema(),
            "bindingId": uuid_schema(),
            "displayName": { "type": "string", "minLength": 1, "maxLength": 63 },
            "contractSchema": {
                "type": "string",
                "enum": [EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1]
            },
            "bindingAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL ExternalKnowledgeBinding contract."
            },
            "bindingDigest": digest_schema(),
            "createdAt": timestamp_schema()
        }),
    )
}

fn external_knowledge_binding_mutation_schema() -> Value {
    object_schema(
        &["externalKnowledgeBinding", "replayed"],
        json!({
            "externalKnowledgeBinding": schema_ref("ExternalKnowledgeBinding"),
            "replayed": { "type": "boolean" }
        }),
    )
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
            "KnowledgeDocument",
            "KnowledgeDocumentMutation",
            "KnowledgeChunk",
            "KnowledgeChunkMutation",
            "KnowledgeIndexRevision",
            "KnowledgeIndexRevisionMutation",
            "KnowledgeRetrievalPolicyRevision",
            "KnowledgeRetrievalPolicyRevisionMutation",
            "ExternalKnowledgeBinding",
            "ExternalKnowledgeBindingMutation",
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
        assert_eq!(
            schemas["KnowledgeDocumentList"]["maxItems"],
            MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT
        );
        assert_eq!(
            schemas["KnowledgeChunkList"]["maxItems"],
            MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT
        );
    }
}
