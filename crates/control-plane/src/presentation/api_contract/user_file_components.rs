use crate::modules::files::{
    MAXIMUM_USER_FILE_LIST_LIMIT, USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
    USER_FILE_ADMISSION_CONTRACT_SCHEMA, USER_FILE_MAX_BYTES, USER_FILE_PUBLIC_INTEGER_MAX,
    USER_FILE_REJECTION_REASON_MAX_BYTES,
};
use serde_json::{json, Map, Value};

const MAXIMUM_JSON_SAFE_INTEGER: u64 = USER_FILE_PUBLIC_INTEGER_MAX;

pub(super) const USER_FILE_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    ("UserFileSuccessResponse", "UserFile"),
    ("UserFileListSuccessResponse", "UserFileList"),
    ("UserFileMutationSuccessResponse", "UserFileMutation"),
    ("UserFileQuotaSuccessResponse", "UserFileQuota"),
];

pub(super) const USER_FILE_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    ("UserFileSuccess200", 200, "UserFileSuccessResponse"),
    ("UserFileListSuccess200", 200, "UserFileListSuccessResponse"),
    (
        "UserFileMutationSuccess200",
        200,
        "UserFileMutationSuccessResponse",
    ),
    (
        "UserFileMutationSuccess201",
        201,
        "UserFileMutationSuccessResponse",
    ),
    (
        "UserFileQuotaSuccess200",
        200,
        "UserFileQuotaSuccessResponse",
    ),
];

pub(super) fn install_user_file_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("UserFile", user_file_schema()),
        ("UserFileList", user_file_list_schema()),
        ("UserFileMutation", user_file_mutation_schema()),
        ("UserFileQuota", user_file_quota_schema()),
    ] {
        schemas.insert(name.into(), schema);
    }
}

fn user_file_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "userFileId",
            "uploadId",
            "state",
            "originalName",
            "contractSchema",
            "admissionAcl",
            "contractDigest",
            "objectRef",
            "contentDigest",
            "sizeBytes",
            "mediaType",
            "scanPolicy",
            "uploadExpiresAt",
            "retentionUntil",
            "scanEvidenceDigest",
            "rejectionReasonCode",
            "tombstonedFrom",
            "aggregateVersion",
            "createdBy",
            "createdAt",
            "uploadedAt",
            "scannedAt",
            "expiredAt",
            "tombstonedAt",
            "cleanupDueAt",
            "updatedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "userFileId": uuid_schema(),
            "uploadId": uuid_schema(),
            "state": {
                "type": "string",
                "enum": [
                    "awaiting_upload", "awaiting_scan", "admitted", "rejected", "expired",
                    "tombstoned"
                ]
            },
            "originalName": {
                "type": "string",
                "minLength": 1,
                "maxLength": 255,
                "pattern": "^[^/\\\\\\u0000-\\u001F\\u007F-\\u009F]+$"
            },
            "contractSchema": {
                "type": "string",
                "enum": [USER_FILE_ADMISSION_CONTRACT_SCHEMA]
            },
            "admissionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
                "x-a3s-max-canonical-bytes": USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
                "description": "Canonical A3S ACL admission contract; this is the only Files product-configuration representation."
            },
            "contractDigest": digest_schema(),
            "objectRef": {
                "type": "string",
                "minLength": 1,
                "maxLength": 512,
                "pattern": "^organizations/[0-9a-f-]{36}/projects/[0-9a-f-]{36}/files/[0-9a-f-]{36}/uploads/[0-9a-f-]{36}/sha256/[0-9a-f]{64}/content$",
                "description": "Logical immutable-object reference without provider, bucket, credential, or local-path details."
            },
            "contentDigest": digest_schema(),
            "sizeBytes": {
                "type": "integer",
                "minimum": 1,
                "maximum": USER_FILE_MAX_BYTES
            },
            "mediaType": {
                "type": "string",
                "minLength": 3,
                "maxLength": 127,
                "pattern": "^[A-Za-z0-9!#$&^_.+-]+/[A-Za-z0-9!#$&^_.+-]+$"
            },
            "scanPolicy": { "type": "string", "enum": ["required"] },
            "uploadExpiresAt": timestamp_schema(false),
            "retentionUntil": timestamp_schema(false),
            "scanEvidenceDigest": nullable_digest_schema(),
            "rejectionReasonCode": {
                "type": "string",
                "minLength": 1,
                "maxLength": USER_FILE_REJECTION_REASON_MAX_BYTES,
                "nullable": true
            },
            "tombstonedFrom": {
                "type": "string",
                "enum": [
                    "awaiting_upload", "awaiting_scan", "admitted", "rejected", "expired", null
                ],
                "nullable": true
            },
            "aggregateVersion": safe_integer_schema(1),
            "createdBy": uuid_schema(),
            "createdAt": timestamp_schema(false),
            "uploadedAt": timestamp_schema(true),
            "scannedAt": timestamp_schema(true),
            "expiredAt": timestamp_schema(true),
            "tombstonedAt": timestamp_schema(true),
            "cleanupDueAt": timestamp_schema(true),
            "updatedAt": timestamp_schema(false)
        }),
    )
}

fn user_file_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_USER_FILE_LIST_LIMIT,
        "items": schema_ref("UserFile")
    })
}

fn user_file_mutation_schema() -> Value {
    object_schema(
        &["file", "replayed"],
        json!({
            "file": schema_ref("UserFile"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn user_file_quota_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "limitBytes",
            "allocatedBytes",
            "availableBytes",
            "revision",
            "updatedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "limitBytes": safe_integer_schema(1),
            "allocatedBytes": safe_integer_schema(0),
            "availableBytes": safe_integer_schema(0),
            "revision": safe_integer_schema(0),
            "updatedAt": timestamp_schema(true)
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

fn nullable_digest_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "^sha256:[0-9a-f]{64}$",
        "nullable": true
    })
}

fn timestamp_schema(nullable: bool) -> Value {
    json!({ "type": "string", "format": "date-time", "nullable": nullable })
}

fn safe_integer_schema(minimum: u64) -> Value {
    json!({
        "type": "integer",
        "minimum": minimum,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_file_schemas_are_closed_bounded_acl_first_and_provider_neutral() {
        let mut schemas = Map::new();
        install_user_file_component_schemas(&mut schemas);

        for name in ["UserFile", "UserFileMutation", "UserFileQuota"] {
            assert_eq!(schemas[name]["additionalProperties"], false, "{name}");
        }
        assert_eq!(
            schemas["UserFile"]["properties"]["admissionAcl"]["maxLength"],
            USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES
        );
        assert_eq!(
            schemas["UserFileList"]["maxItems"],
            MAXIMUM_USER_FILE_LIST_LIMIT
        );
        let properties = schemas["UserFile"]["properties"]
            .as_object()
            .expect("UserFile properties");
        for forbidden in [
            "bytes",
            "provider",
            "bucket",
            "credential",
            "multipart",
            "scannerProvider",
        ] {
            assert!(!properties.contains_key(forbidden));
        }
    }
}
