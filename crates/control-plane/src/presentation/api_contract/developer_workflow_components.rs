use super::source_components::build_recipe_response_schema;
use super::workflow_components::{digest_schema, timestamp_schema, uuid_schema};
use crate::modules::developer_workflows::{
    BuildPlanDetectionDiagnosticCode, BuildPlanDetectorKind, BUILD_PLAN_DETECTOR_REVISION,
    BUILD_PLAN_MAX_ACL_BYTES, BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES, BUILD_PLAN_PROPOSAL_SCHEMA,
    BUILD_PLAN_SCHEMA, MAXIMUM_BUILD_PLAN_LIST_LIMIT, MAX_BUILD_PLAN_DIAGNOSTICS,
    MAX_BUILD_PLAN_PROPOSALS, MAX_SOURCE_LAYOUT_PATH_BYTES,
};
use crate::modules::sources::published::BuildRecipe;
use serde_json::{json, Map, Value};

const BUILD_PLAN_PROPOSAL_ACL_EXAMPLE: &str =
    include_str!("../../../../../contracts/p0.1/build-plan.acl");
const ACCEPTED_BUILD_PLAN_ACL_EXAMPLE: &str =
    include_str!("../../../../../contracts/p0.1/accepted-build-plan.acl");

pub(super) const BUILD_PLAN_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    ("BuildPlanDetectionSuccessResponse", "BuildPlanDetection"),
    ("AcceptedBuildPlanSuccessResponse", "AcceptedBuildPlan"),
    (
        "AcceptedBuildPlanListSuccessResponse",
        "AcceptedBuildPlanList",
    ),
    ("BuildPlanMutationSuccessResponse", "BuildPlanMutation"),
];

pub(super) const BUILD_PLAN_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "BuildPlanDetectionSuccess200",
        200,
        "BuildPlanDetectionSuccessResponse",
    ),
    (
        "AcceptedBuildPlanSuccess200",
        200,
        "AcceptedBuildPlanSuccessResponse",
    ),
    (
        "AcceptedBuildPlanListSuccess200",
        200,
        "AcceptedBuildPlanListSuccessResponse",
    ),
    (
        "BuildPlanMutationSuccess200",
        200,
        "BuildPlanMutationSuccessResponse",
    ),
    (
        "BuildPlanMutationSuccess201",
        201,
        "BuildPlanMutationSuccessResponse",
    ),
];

pub(super) fn install_developer_workflow_component_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert("BuildPlanSource".into(), build_plan_source_schema());
    schemas.insert("BuildRecipe".into(), build_recipe_response_schema());
    schemas.insert("BuildPlanProposal".into(), build_plan_proposal_schema());
    schemas.insert(
        "BuildPlanDetectionDiagnostic".into(),
        build_plan_detection_diagnostic_schema(),
    );
    schemas.insert("BuildPlanDetection".into(), build_plan_detection_schema());
    schemas.insert("AcceptedBuildPlan".into(), accepted_build_plan_schema());
    schemas.insert(
        "AcceptedBuildPlanList".into(),
        accepted_build_plan_list_schema(),
    );
    schemas.insert("BuildPlanMutation".into(), build_plan_mutation_schema());
}

pub(super) fn detect_build_plans_request_schema() -> Value {
    object_schema(
        &["sourceRevisionId"],
        json!({ "sourceRevisionId": uuid_schema() }),
    )
}

pub(super) fn accept_build_plan_request_schema() -> Value {
    object_schema(
        &["sourceRevisionId", "proposalAcl"],
        json!({
            "sourceRevisionId": uuid_schema(),
            "proposalAcl": canonical_acl_schema(
                BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES,
                BUILD_PLAN_PROPOSAL_ACL_EXAMPLE,
            )
        }),
    )
}

fn build_plan_source_schema() -> Value {
    object_schema(
        &["sourceIdentityDigest", "commitSha", "sourceContentDigest"],
        json!({
            "sourceIdentityDigest": digest_schema(),
            "commitSha": {
                "type": "string",
                "pattern": "^(?:[0-9a-f]{40}|[0-9a-f]{64})$"
            },
            "sourceContentDigest": digest_schema()
        }),
    )
}

fn build_plan_proposal_schema() -> Value {
    object_schema(
        &[
            "schema",
            "proposalAcl",
            "proposalDigest",
            "detector",
            "detectorRevision",
            "projectRoot",
            "evidencePath",
            "evidenceDigest",
            "recipe",
        ],
        json!({
            "schema": { "type": "string", "enum": [BUILD_PLAN_PROPOSAL_SCHEMA] },
            "proposalAcl": canonical_acl_schema(
                BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES,
                BUILD_PLAN_PROPOSAL_ACL_EXAMPLE,
            ),
            "proposalDigest": digest_schema(),
            "detector": {
                "type": "string",
                "enum": [
                    BuildPlanDetectorKind::AssetAcl.as_str(),
                    BuildPlanDetectorKind::Dockerfile.as_str()
                ]
            },
            "detectorRevision": {
                "type": "string",
                "enum": [BUILD_PLAN_DETECTOR_REVISION]
            },
            "projectRoot": repository_path_schema(BuildRecipe::MAX_REPOSITORY_PATH_BYTES),
            "evidencePath": repository_path_schema(BuildRecipe::MAX_REPOSITORY_PATH_BYTES),
            "evidenceDigest": digest_schema(),
            "recipe": schema_ref("BuildRecipe")
        }),
    )
}

fn build_plan_detection_diagnostic_schema() -> Value {
    object_schema(
        &["code", "path"],
        json!({
            "code": {
                "type": "string",
                "enum": [
                    BuildPlanDetectionDiagnosticCode::AssetBuildRecipeMissing.as_str(),
                    BuildPlanDetectionDiagnosticCode::EmptyDockerfile.as_str(),
                    BuildPlanDetectionDiagnosticCode::NoSupportedLayout.as_str()
                ]
            },
            "path": {
                "allOf": [repository_path_schema(MAX_SOURCE_LAYOUT_PATH_BYTES)],
                "nullable": true
            }
        }),
    )
}

fn build_plan_detection_schema() -> Value {
    object_schema(
        &["source", "proposals", "diagnostics"],
        json!({
            "source": schema_ref("BuildPlanSource"),
            "proposals": {
                "type": "array",
                "maxItems": MAX_BUILD_PLAN_PROPOSALS,
                "uniqueItems": true,
                "x-a3s-canonical-order": ["projectRoot", "detector", "proposalDigest"],
                "items": schema_ref("BuildPlanProposal")
            },
            "diagnostics": {
                "type": "array",
                "maxItems": MAX_BUILD_PLAN_DIAGNOSTICS,
                "uniqueItems": true,
                "x-a3s-canonical-order": ["code", "path"],
                "items": schema_ref("BuildPlanDetectionDiagnostic")
            }
        }),
    )
}

fn accepted_build_plan_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "environmentId",
            "buildPlanId",
            "sourceRevisionId",
            "contractSchema",
            "contractAcl",
            "contractDigest",
            "proposal",
            "aggregateVersion",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "environmentId": uuid_schema(),
            "buildPlanId": uuid_schema(),
            "sourceRevisionId": uuid_schema(),
            "contractSchema": { "type": "string", "enum": [BUILD_PLAN_SCHEMA] },
            "contractAcl": canonical_acl_schema(
                BUILD_PLAN_MAX_ACL_BYTES,
                ACCEPTED_BUILD_PLAN_ACL_EXAMPLE,
            ),
            "contractDigest": digest_schema(),
            "proposal": schema_ref("BuildPlanProposal"),
            "aggregateVersion": {
                "type": "integer",
                "format": "int64",
                "enum": [1]
            },
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn accepted_build_plan_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_BUILD_PLAN_LIST_LIMIT,
        "uniqueItems": true,
        "x-a3s-canonical-order": ["proposal.projectRoot", "buildPlanId"],
        "items": schema_ref("AcceptedBuildPlan")
    })
}

fn build_plan_mutation_schema() -> Value {
    object_schema(
        &["buildPlan", "replayed"],
        json!({
            "buildPlan": schema_ref("AcceptedBuildPlan"),
            "replayed": { "type": "boolean" }
        }),
    )
}

pub(super) fn canonical_acl_schema(max_length: usize, example: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": max_length,
        "x-a3s-max-utf8-bytes": max_length,
        "description": "Canonical A3S ACL parsed and generated only through a3s-acl.",
        "example": example
    })
}

pub(super) fn repository_path_schema(max_length: usize) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": max_length,
        "x-a3s-max-utf8-bytes": max_length,
        "description": "Canonical relative POSIX repository path; '.' denotes the repository root where permitted."
    })
}

pub(super) fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

pub(super) fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_set_is_closed_bounded_and_acl_only() {
        assert_eq!(BUILD_PLAN_SUCCESS_SCHEMA_BINDINGS.len(), 4);
        assert_eq!(BUILD_PLAN_SUCCESS_RESPONSE_BINDINGS.len(), 5);
        let mut schemas = Map::new();
        install_developer_workflow_component_schemas(&mut schemas);
        assert_eq!(schemas.len(), 8);
        for name in [
            "BuildPlanSource",
            "BuildRecipe",
            "BuildPlanProposal",
            "BuildPlanDetectionDiagnostic",
            "BuildPlanDetection",
            "AcceptedBuildPlan",
            "BuildPlanMutation",
        ] {
            assert_eq!(
                schemas[name]["additionalProperties"].as_bool(),
                Some(false),
                "{name} must remain closed"
            );
        }

        let proposal = &schemas["BuildPlanProposal"]["properties"];
        assert_eq!(
            proposal["proposalAcl"]["maxLength"].as_u64(),
            Some(BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES as u64)
        );
        assert_eq!(
            proposal["proposalAcl"]["x-a3s-max-utf8-bytes"].as_u64(),
            Some(BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES as u64)
        );
        assert_eq!(
            proposal["proposalAcl"]["example"].as_str(),
            Some(BUILD_PLAN_PROPOSAL_ACL_EXAMPLE)
        );
        assert!(proposal.get("proposal").is_none());
        assert!(proposal.get("sourceBytes").is_none());

        let accepted = &schemas["AcceptedBuildPlan"]["properties"];
        assert_eq!(
            accepted["contractAcl"]["maxLength"].as_u64(),
            Some(BUILD_PLAN_MAX_ACL_BYTES as u64)
        );
        assert_eq!(accepted["aggregateVersion"]["enum"], json!([1]));
        assert_eq!(
            accepted["contractAcl"]["example"].as_str(),
            Some(ACCEPTED_BUILD_PLAN_ACL_EXAMPLE)
        );
        for forbidden in ["checkoutPath", "credentials", "sourceBytes"] {
            assert!(accepted.get(forbidden).is_none());
        }
        assert_eq!(
            schemas["AcceptedBuildPlanList"]["maxItems"].as_u64(),
            Some(MAXIMUM_BUILD_PLAN_LIST_LIMIT as u64)
        );

        let detection_request = detect_build_plans_request_schema();
        assert_eq!(
            detection_request["additionalProperties"].as_bool(),
            Some(false)
        );
        assert!(detection_request["properties"].get("sourceBytes").is_none());

        let acceptance_request = accept_build_plan_request_schema();
        let acceptance_properties = &acceptance_request["properties"];
        assert_eq!(
            acceptance_properties["proposalAcl"]["maxLength"].as_u64(),
            Some(BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES as u64)
        );
        assert!(acceptance_properties.get("proposal").is_none());
    }
}
