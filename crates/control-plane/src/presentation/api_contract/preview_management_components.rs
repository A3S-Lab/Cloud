use super::developer_workflow_components::{canonical_acl_schema, object_schema, schema_ref};
use super::workflow_components::{digest_schema, timestamp_schema, uuid_schema};
use crate::modules::developer_workflows::{
    MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT, MAX_ACTIVE_PREVIEWS_PER_POLICY,
    MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER, MAX_PREVIEW_ENVIRONMENT_NAME_BYTES,
    MAX_PREVIEW_GIT_BRANCH_BYTES, MAX_PREVIEW_GIT_REPOSITORY_URL_BYTES,
    MAX_PREVIEW_LIFETIME_SECONDS, MIN_PREVIEW_LIFETIME_SECONDS, PREVIEW_MAX_CPU_MILLIS,
    PREVIEW_MAX_MEMORY_BYTES, PREVIEW_MAX_STORAGE_BYTES, PREVIEW_MAX_WORKLOADS,
    PREVIEW_MIN_MEMORY_BYTES, PREVIEW_MIN_STORAGE_BYTES, PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES,
    PULL_REQUEST_PREVIEW_POLICY_SCHEMA,
};
use serde_json::{json, Map, Value};

const PREVIEW_POLICY_ACL_EXAMPLE: &str =
    include_str!("../../../../../contracts/p0.3/pull-request-preview-policy.acl");

pub(super) const PREVIEW_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    (
        "AcceptedPullRequestPreviewPolicyRevisionSuccessResponse",
        "AcceptedPullRequestPreviewPolicyRevision",
    ),
    (
        "AcceptedPullRequestPreviewPolicyRevisionListSuccessResponse",
        "AcceptedPullRequestPreviewPolicyRevisionList",
    ),
    (
        "PullRequestPreviewPolicyMutationSuccessResponse",
        "PullRequestPreviewPolicyMutation",
    ),
    ("PullRequestPreviewSuccessResponse", "PullRequestPreview"),
];

pub(super) const PREVIEW_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "AcceptedPullRequestPreviewPolicyRevisionSuccess200",
        200,
        "AcceptedPullRequestPreviewPolicyRevisionSuccessResponse",
    ),
    (
        "AcceptedPullRequestPreviewPolicyRevisionListSuccess200",
        200,
        "AcceptedPullRequestPreviewPolicyRevisionListSuccessResponse",
    ),
    (
        "PullRequestPreviewPolicyMutationSuccess200",
        200,
        "PullRequestPreviewPolicyMutationSuccessResponse",
    ),
    (
        "PullRequestPreviewPolicyMutationSuccess201",
        201,
        "PullRequestPreviewPolicyMutationSuccessResponse",
    ),
    (
        "PullRequestPreviewSuccess200",
        200,
        "PullRequestPreviewSuccessResponse",
    ),
];

pub(super) fn install_preview_management_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("PreviewGitRepository", preview_git_repository_schema()),
        ("PreviewQuota", preview_quota_schema()),
        (
            "PullRequestPreviewPolicy",
            pull_request_preview_policy_schema(),
        ),
        (
            "AcceptedPullRequestPreviewPolicyRevision",
            accepted_preview_policy_revision_schema(),
        ),
        (
            "AcceptedPullRequestPreviewPolicyRevisionList",
            accepted_preview_policy_revision_list_schema(),
        ),
        (
            "PullRequestPreviewPolicyMutation",
            preview_policy_mutation_schema(),
        ),
        ("PullRequestPreview", pull_request_preview_schema()),
    ] {
        schemas.insert(name.into(), schema);
    }
}

pub(super) fn accept_preview_policy_request_schema() -> Value {
    object_schema(
        &["sourceSubscriptionId", "policyAcl"],
        json!({
            "sourceSubscriptionId": uuid_schema(),
            "policyAcl": canonical_acl_schema(
                PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES,
                PREVIEW_POLICY_ACL_EXAMPLE,
            )
        }),
    )
}

fn preview_git_repository_schema() -> Value {
    object_schema(
        &["provider", "canonicalUrl"],
        json!({
            "provider": { "type": "string", "enum": ["github"] },
            "canonicalUrl": {
                "type": "string",
                "format": "uri",
                "minLength": 1,
                "maxLength": MAX_PREVIEW_GIT_REPOSITORY_URL_BYTES,
                "x-a3s-max-utf8-bytes": MAX_PREVIEW_GIT_REPOSITORY_URL_BYTES,
                "pattern": "^https://github\\.com/(?:[a-z0-9]|(?![a-z0-9-]*--)[a-z0-9][a-z0-9-]{0,37}[a-z0-9])/(?!\\.{1,2}$)[a-z0-9._-]{1,100}$"
            }
        }),
    )
}

fn preview_quota_schema() -> Value {
    object_schema(
        &[
            "maximumWorkloads",
            "cpuMillis",
            "memoryBytes",
            "ephemeralStorageBytes",
        ],
        json!({
            "maximumWorkloads": {
                "type": "integer", "minimum": 1, "maximum": PREVIEW_MAX_WORKLOADS
            },
            "cpuMillis": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": PREVIEW_MAX_CPU_MILLIS
            },
            "memoryBytes": {
                "type": "integer", "format": "int64",
                "minimum": PREVIEW_MIN_MEMORY_BYTES, "maximum": PREVIEW_MAX_MEMORY_BYTES,
                "multipleOf": 1_048_576
            },
            "ephemeralStorageBytes": {
                "type": "integer", "format": "int64",
                "minimum": PREVIEW_MIN_STORAGE_BYTES, "maximum": PREVIEW_MAX_STORAGE_BYTES,
                "multipleOf": 1_048_576
            }
        }),
    )
}

fn pull_request_preview_policy_schema() -> Value {
    object_schema(
        &[
            "ownerPrincipalId",
            "installationId",
            "baseRepository",
            "baseBranch",
            "lifetimeSeconds",
            "maximumActivePreviews",
            "forkPolicy",
            "allowProtectedSecretsForTrustedSources",
            "quota",
        ],
        json!({
            "ownerPrincipalId": uuid_schema(),
            "installationId": portable_positive_integer_schema(),
            "baseRepository": schema_ref("PreviewGitRepository"),
            "baseBranch": preview_git_branch_schema(),
            "lifetimeSeconds": {
                "type": "integer", "format": "int32",
                "minimum": MIN_PREVIEW_LIFETIME_SECONDS,
                "maximum": MAX_PREVIEW_LIFETIME_SECONDS
            },
            "maximumActivePreviews": {
                "type": "integer", "minimum": 1,
                "maximum": MAX_ACTIVE_PREVIEWS_PER_POLICY
            },
            "forkPolicy": { "type": "string", "enum": ["deny", "isolated"] },
            "allowProtectedSecretsForTrustedSources": { "type": "boolean" },
            "quota": schema_ref("PreviewQuota")
        }),
    )
}

fn accepted_preview_policy_revision_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "sourceEnvironmentId",
            "sourceSubscriptionId",
            "pullRequestPreviewPolicyRevisionId",
            "revisionNumber",
            "contractSchema",
            "contractAcl",
            "contractDigest",
            "policy",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "sourceEnvironmentId": uuid_schema(),
            "sourceSubscriptionId": uuid_schema(),
            "pullRequestPreviewPolicyRevisionId": uuid_schema(),
            "revisionNumber": portable_positive_integer_schema(),
            "contractSchema": {
                "type": "string", "enum": [PULL_REQUEST_PREVIEW_POLICY_SCHEMA]
            },
            "contractAcl": canonical_acl_schema(
                PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES,
                PREVIEW_POLICY_ACL_EXAMPLE,
            ),
            "contractDigest": digest_schema(),
            "policy": schema_ref("PullRequestPreviewPolicy"),
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn accepted_preview_policy_revision_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT,
        "uniqueItems": true,
        "x-a3s-canonical-order": ["revisionNumber", "pullRequestPreviewPolicyRevisionId"],
        "items": schema_ref("AcceptedPullRequestPreviewPolicyRevision")
    })
}

fn preview_policy_mutation_schema() -> Value {
    object_schema(
        &["previewPolicyRevision", "replayed"],
        json!({
            "previewPolicyRevision": schema_ref("AcceptedPullRequestPreviewPolicyRevision"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn pull_request_preview_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "sourceEnvironmentId",
            "sourceSubscriptionId",
            "previewId",
            "environmentId",
            "environmentName",
            "pullRequestId",
            "pullRequestNumber",
            "policyRevisionId",
            "policyRevisionNumber",
            "policyAcceptedAt",
            "policy",
            "headRepository",
            "headBranch",
            "headCommitSha",
            "providerCreatedAt",
            "lastProviderUpdatedAt",
            "lastChangeKind",
            "lastMerged",
            "expiresAt",
            "status",
            "cleanupReason",
            "cleanupRequestedAt",
            "aggregateVersion",
            "isFork",
            "protectedSecretsEligible",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "sourceEnvironmentId": uuid_schema(),
            "sourceSubscriptionId": uuid_schema(),
            "previewId": uuid_schema(),
            "environmentId": uuid_schema(),
            "environmentName": {
                "type": "string", "minLength": 1,
                "maxLength": MAX_PREVIEW_ENVIRONMENT_NAME_BYTES,
                "pattern": "^pr-[1-9][0-9]*-[0-9a-f]{32}$"
            },
            "pullRequestId": portable_positive_integer_schema(),
            "pullRequestNumber": portable_positive_integer_schema(),
            "policyRevisionId": uuid_schema(),
            "policyRevisionNumber": portable_positive_integer_schema(),
            "policyAcceptedAt": timestamp_schema(),
            "policy": schema_ref("PullRequestPreviewPolicy"),
            "headRepository": {
                "allOf": [schema_ref("PreviewGitRepository")], "nullable": true
            },
            "headBranch": preview_git_branch_schema(),
            "headCommitSha": {
                "type": "string", "pattern": "^(?:[0-9a-f]{40}|[0-9a-f]{64})$"
            },
            "providerCreatedAt": timestamp_schema(),
            "lastProviderUpdatedAt": timestamp_schema(),
            "lastChangeKind": {
                "type": "string", "enum": ["opened", "synchronized", "reopened", "closed"]
            },
            "lastMerged": { "type": "boolean" },
            "expiresAt": timestamp_schema(),
            "status": { "type": "string", "enum": ["active", "cleanup_required"] },
            "cleanupReason": {
                "type": "string",
                "enum": ["pull_request_closed", "pull_request_merged", "fork_denied", "expired"],
                "nullable": true
            },
            "cleanupRequestedAt": {
                "allOf": [timestamp_schema()], "nullable": true
            },
            "aggregateVersion": portable_positive_integer_schema(),
            "isFork": { "type": "boolean" },
            "protectedSecretsEligible": { "type": "boolean" }
        }),
    )
}

fn portable_positive_integer_schema() -> Value {
    json!({
        "type": "integer",
        "format": "int64",
        "minimum": 1,
        "maximum": MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER
    })
}

fn preview_git_branch_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_PREVIEW_GIT_BRANCH_BYTES,
        "x-a3s-max-utf8-bytes": MAX_PREVIEW_GIT_BRANCH_BYTES,
        "pattern": "^(?!refs/)(?!/)(?!.*\\.(?:/|$))(?!.*\\/$)(?!.*//)(?!.*\\.\\.)(?!.*(?:^|/)\\.)(?!.*\\.lock(?:/|$))[A-Za-z0-9_.\\/-]+$"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_management_components_are_closed_bounded_and_acl_only() {
        assert_eq!(PREVIEW_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS.len(), 4);
        assert_eq!(PREVIEW_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS.len(), 5);
        let mut schemas = Map::new();
        install_preview_management_component_schemas(&mut schemas);
        assert_eq!(schemas.len(), 7);
        for name in [
            "PreviewGitRepository",
            "PreviewQuota",
            "PullRequestPreviewPolicy",
            "AcceptedPullRequestPreviewPolicyRevision",
            "PullRequestPreviewPolicyMutation",
            "PullRequestPreview",
        ] {
            assert_eq!(schemas[name]["additionalProperties"], false, "{name}");
        }
        assert_eq!(
            schemas["AcceptedPullRequestPreviewPolicyRevision"]["properties"]["contractAcl"]
                ["maxLength"],
            PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES
        );
        assert_eq!(
            schemas["PullRequestPreview"]["properties"]["pullRequestId"]["maximum"],
            MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER
        );
        assert_eq!(
            schemas["PullRequestPreview"]["properties"]["headBranch"],
            schemas["PullRequestPreviewPolicy"]["properties"]["baseBranch"]
        );
        assert_eq!(
            schemas["AcceptedPullRequestPreviewPolicyRevisionList"]["maxItems"],
            MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT
        );
        let encoded = Value::Object(schemas).to_string();
        for forbidden in [
            "webhookSecret",
            "signature",
            "deliveryBody",
            "credential",
            "providerToken",
        ] {
            assert!(!encoded.contains(forbidden));
        }

        let request = accept_preview_policy_request_schema();
        assert_eq!(
            request["required"],
            json!(["sourceSubscriptionId", "policyAcl"])
        );
        assert_eq!(request["additionalProperties"], false);
    }
}
