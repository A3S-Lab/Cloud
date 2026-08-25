use super::workflow_components::{
    digest_schema, nullable_digest_schema, nullable_uuid_schema, revision_number_schema,
    timestamp_schema, uuid_schema,
};
use crate::modules::workflow::{
    ONTOLOGY_COMPILER_SCHEMA_VERSION, ONTOLOGY_MAX_ACL_BYTES, ONTOLOGY_SCHEMA,
};
use serde_json::{json, Map, Value};

pub(super) fn install_workflow_ontology_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert("Ontology".into(), ontology_schema());
    schemas.insert(
        "OntologyList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/Ontology" }
        }),
    );
    schemas.insert(
        "OntologyMigrationPolicy".into(),
        ontology_migration_policy_schema(),
    );
    schemas.insert(
        "OntologyRevisionSummary".into(),
        ontology_revision_summary_schema(),
    );
    schemas.insert(
        "OntologyRevisionSummaryList".into(),
        json!({
            "type": "array",
            "items": { "$ref": "#/components/schemas/OntologyRevisionSummary" }
        }),
    );
    schemas.insert("OntologyRevision".into(), ontology_revision_schema());
    schemas.insert("OntologyChange".into(), ontology_change_schema());
    schemas.insert("OntologyDiff".into(), ontology_diff_schema());
    schemas.insert(
        "OntologyRevisionDiff".into(),
        ontology_revision_diff_schema(),
    );
    schemas.insert("OntologyMutation".into(), ontology_mutation_schema());
}

fn ontology_schema() -> Value {
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

fn ontology_migration_policy_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "ruleId", "expressionDigest"],
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["initial", "compatible", "explicit"]
            },
            "ruleId": {
                "type": "string",
                "minLength": 1,
                "maxLength": 96,
                "pattern": "^[A-Za-z0-9_-]+$",
                "nullable": true
            },
            "expressionDigest": nullable_digest_schema()
        }
    })
}

fn ontology_revision_summary_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ontology_revision_required_fields(false),
        "properties": ontology_revision_properties(false)
    })
}

fn ontology_revision_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ontology_revision_required_fields(true),
        "properties": ontology_revision_properties(true)
    })
}

fn ontology_revision_required_fields(include_acl: bool) -> Vec<&'static str> {
    let mut required = vec![
        "organizationId",
        "projectId",
        "ontologyId",
        "id",
        "revisionNumber",
        "parentRevisionId",
        "parentDigest",
        "contractSchema",
        "compilerSchemaVersion",
        "contentDigest",
        "migrationPolicy",
        "createdBy",
        "createdAt",
    ];
    if include_acl {
        required.push("canonicalAcl");
    }
    required
}

fn ontology_revision_properties(include_acl: bool) -> Map<String, Value> {
    let mut properties = [
        ("organizationId", uuid_schema()),
        ("projectId", uuid_schema()),
        ("ontologyId", uuid_schema()),
        ("id", uuid_schema()),
        ("revisionNumber", revision_number_schema()),
        ("parentRevisionId", nullable_uuid_schema()),
        ("parentDigest", nullable_digest_schema()),
        (
            "contractSchema",
            json!({ "type": "string", "enum": [ONTOLOGY_SCHEMA] }),
        ),
        (
            "compilerSchemaVersion",
            json!({ "type": "integer", "enum": [ONTOLOGY_COMPILER_SCHEMA_VERSION] }),
        ),
        ("contentDigest", digest_schema()),
        (
            "migrationPolicy",
            json!({ "$ref": "#/components/schemas/OntologyMigrationPolicy" }),
        ),
        ("createdBy", uuid_schema()),
        ("createdAt", timestamp_schema()),
    ]
    .into_iter()
    .map(|(name, schema)| (name.to_owned(), schema))
    .collect::<Map<_, _>>();
    if include_acl {
        properties.insert(
            "canonicalAcl".into(),
            json!({
                "type": "string",
                "minLength": 1,
                "maxLength": ONTOLOGY_MAX_ACL_BYTES
            }),
        );
    }
    properties
}

fn ontology_change_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "resourceKind", "resourceId", "changeKind", "compatibility", "changedFields"
        ],
        "properties": {
            "resourceKind": {
                "type": "string",
                "enum": ["metadata", "object_type", "relation_type", "rule"]
            },
            "resourceId": {
                "type": "string",
                "minLength": 1,
                "maxLength": 96,
                "pattern": "^[A-Za-z0-9_-]+$"
            },
            "changeKind": {
                "type": "string",
                "enum": ["added", "removed", "changed"]
            },
            "compatibility": {
                "type": "string",
                "enum": ["compatible", "breaking"]
            },
            "changedFields": {
                "type": "array",
                "uniqueItems": true,
                "items": {
                    "type": "string",
                    "enum": [
                        "name", "description", "label", "schema_digest", "key_fields",
                        "source_object", "target_object", "cardinality", "kind",
                        "expression_digest"
                    ]
                }
            }
        }
    })
}

fn ontology_diff_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["fromDigest", "toDigest", "breaking", "changes"],
        "properties": ontology_diff_properties()
    })
}

fn ontology_revision_diff_schema() -> Value {
    let mut properties = ontology_diff_properties();
    properties.insert("ontologyId".into(), uuid_schema());
    properties.insert("fromRevisionId".into(), uuid_schema());
    properties.insert("toRevisionId".into(), uuid_schema());
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "ontologyId", "fromRevisionId", "toRevisionId",
            "fromDigest", "toDigest", "breaking", "changes"
        ],
        "properties": properties
    })
}

fn ontology_diff_properties() -> Map<String, Value> {
    [
        ("fromDigest", digest_schema()),
        ("toDigest", digest_schema()),
        ("breaking", json!({ "type": "boolean" })),
        (
            "changes",
            json!({
                "type": "array",
                "items": { "$ref": "#/components/schemas/OntologyChange" }
            }),
        ),
    ]
    .into_iter()
    .map(|(name, schema)| (name.to_owned(), schema))
    .collect()
}

fn ontology_mutation_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ontology", "revision", "diff", "replayed"],
        "properties": {
            "ontology": { "$ref": "#/components/schemas/Ontology" },
            "revision": { "$ref": "#/components/schemas/OntologyRevision" },
            "diff": {
                "type": "object",
                "nullable": true,
                "allOf": [{ "$ref": "#/components/schemas/OntologyDiff" }]
            },
            "replayed": { "type": "boolean" }
        }
    })
}
