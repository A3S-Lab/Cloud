use super::*;
use crate::modules::developer_workflows::{
    BUILD_PLAN_COLLECTION_ROUTE, BUILD_PLAN_DETECTION_ROUTE, BUILD_PLAN_ITEM_ROUTE,
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX, PULL_REQUEST_PREVIEW_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE, PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE, WORKLOAD_PROFILE_COLLECTION_ROUTE,
    WORKLOAD_PROFILE_ITEM_ROUTE, WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
    WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
};
use crate::modules::sources::published::BuildRecipe;

fn full_route(route: &str) -> String {
    format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
}

#[test]
fn build_plan_schemas_are_closed_bounded_typed_and_acl_only() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

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
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["BuildPlanProposal"]["properties"]["proposalAcl"]["maxLength"],
        crate::modules::developer_workflows::BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES
    );
    assert_eq!(
        schemas["BuildPlanProposal"]["properties"]["proposalAcl"]["x-a3s-max-utf8-bytes"],
        crate::modules::developer_workflows::BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES
    );
    assert_eq!(
        schemas["BuildRecipe"]["properties"]["contextPath"]["maxLength"],
        BuildRecipe::MAX_REPOSITORY_PATH_BYTES
    );
    assert_eq!(
        schemas["BuildRecipe"]["properties"]["target"]["maxLength"],
        BuildRecipe::MAX_TARGET_BYTES
    );
    let source_revision_recipe = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"]
        ["post"]["requestBody"]["content"]["application/json"]["schema"]["properties"]
        ["recipe"];
    assert_eq!(
        schema_without_documentation(&source_revision_recipe["properties"]),
        schema_without_documentation(&schemas["BuildRecipe"]["properties"]),
        "Sources and Developer Workflows must publish one BuildRecipe property contract"
    );
    assert!(!source_revision_recipe["required"]
        .as_array()
        .expect("source request required fields")
        .contains(&json!("target")));
    assert!(schemas["BuildRecipe"]["required"]
        .as_array()
        .expect("BuildRecipe response required fields")
        .contains(&json!("target")));
    assert_eq!(
        schemas["AcceptedBuildPlan"]["properties"]["contractAcl"]["maxLength"],
        crate::modules::developer_workflows::BUILD_PLAN_MAX_ACL_BYTES
    );
    assert_eq!(
        schemas["AcceptedBuildPlan"]["properties"]["aggregateVersion"]["enum"],
        json!([1])
    );
    assert_eq!(
        schemas["AcceptedBuildPlanList"]["maxItems"],
        crate::modules::developer_workflows::MAXIMUM_BUILD_PLAN_LIST_LIMIT
    );
    assert_eq!(
        schemas["AcceptedBuildPlanList"]["x-a3s-canonical-order"],
        json!(["proposal.projectRoot", "buildPlanId"])
    );

    for (success_schema, data_schema) in [
        ("BuildPlanDetectionSuccessResponse", "BuildPlanDetection"),
        ("AcceptedBuildPlanSuccessResponse", "AcceptedBuildPlan"),
        (
            "AcceptedBuildPlanListSuccessResponse",
            "AcceptedBuildPlanList",
        ),
        ("BuildPlanMutationSuccessResponse", "BuildPlanMutation"),
    ] {
        assert_eq!(
            schemas[success_schema]["allOf"][0]["properties"]["data"]["$ref"],
            format!("#/components/schemas/{data_schema}")
        );
    }

    let build_plan_schemas = serde_json::to_string(&json!({
        "source": schemas["BuildPlanSource"],
        "recipe": schemas["BuildRecipe"],
        "proposal": schemas["BuildPlanProposal"],
        "detection": schemas["BuildPlanDetection"],
        "accepted": schemas["AcceptedBuildPlan"],
    }))
    .map_err(|error| BootError::Internal(error.to_string()))?;
    for forbidden in [
        "sourceBytes",
        "checkoutPath",
        "credentials",
        "docker-compose",
        "compose.yaml",
        "compose.yml",
    ] {
        assert!(
            !build_plan_schemas.contains(forbidden),
            "BuildPlan schemas must not expose {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn build_plan_routes_use_one_typed_public_contract() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let detection_path = full_route(BUILD_PLAN_DETECTION_ROUTE);
    let collection_path = full_route(BUILD_PLAN_COLLECTION_ROUTE);
    let item_path = full_route(BUILD_PLAN_ITEM_ROUTE);
    let detection = &document["paths"][detection_path]["post"];
    let collection = &document["paths"][collection_path];
    let item = &document["paths"][item_path]["get"];

    for operation in [detection, &collection["get"], &collection["post"], item] {
        assert_eq!(operation["tags"], json!(["Developer Workflows"]));
    }

    assert_eq!(
        detection["requestBody"]["content"]["application/json"]["schema"]["required"],
        json!(["sourceRevisionId"])
    );
    assert_eq!(
        collection["post"]["requestBody"]["content"]["application/json"]["schema"]["required"],
        json!(["sourceRevisionId", "proposalAcl"])
    );
    assert_eq!(
        collection["post"]["requestBody"]["content"]["application/json"]["schema"]["properties"]
            ["proposalAcl"]["maxLength"],
        crate::modules::developer_workflows::BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES
    );

    assert!(!has_parameter(detection, "idempotency-key"));
    assert!(has_parameter(&collection["post"], "idempotency-key"));
    let source_revision = parameter(&collection["get"], "sourceRevisionId")?;
    assert_eq!(source_revision["required"], true);
    assert_eq!(source_revision["schema"]["format"], "uuid");
    let limit = parameter(&collection["get"], "limit")?;
    assert_eq!(limit["schema"]["default"], 50);
    assert_eq!(
        limit["schema"]["maximum"],
        crate::modules::developer_workflows::MAXIMUM_BUILD_PLAN_LIST_LIMIT
    );

    assert_eq!(
        detection["responses"]["200"]["$ref"],
        "#/components/responses/BuildPlanDetectionSuccess200"
    );
    assert_eq!(
        collection["get"]["responses"]["200"]["$ref"],
        "#/components/responses/AcceptedBuildPlanListSuccess200"
    );
    assert_eq!(
        item["responses"]["200"]["$ref"],
        "#/components/responses/AcceptedBuildPlanSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/BuildPlanMutationSuccess{status}")
        );
    }
    for operation in [detection, &collection["post"]] {
        assert!(operation["responses"]["413"].is_object());
        assert!(operation["responses"]["415"].is_object());
    }
    Ok(())
}

#[test]
fn workload_profile_schemas_are_closed_typed_revisioned_and_acl_only() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    for name in [
        "WorkloadProcess",
        "WorkloadSecretEnvironmentTarget",
        "WorkloadSecretFileTarget",
        "WorkloadSecretRegistryCredentialTarget",
        "WorkloadSecretBinding",
        "WorkloadProfileResources",
        "WorkloadServicePort",
        "WorkloadHttpHealthCheck",
        "ScheduledTaskRetryPolicy",
        "ScheduledTaskHistoryPolicy",
        "ScheduledTaskSchedule",
        "WorkloadProfileSpec",
        "AcceptedWorkloadProfileRevision",
        "WorkloadProfileMutation",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["AcceptedWorkloadProfileRevision"]["properties"]["contractAcl"]["maxLength"],
        crate::modules::developer_workflows::WORKLOAD_PROFILE_MAX_ACL_BYTES
    );
    assert_eq!(
        schemas["AcceptedWorkloadProfileRevision"]["properties"]["contractSchema"]["enum"],
        json!([crate::modules::developer_workflows::WORKLOAD_PROFILE_SCHEMA])
    );
    assert_eq!(
        schemas["AcceptedWorkloadProfileRevisionList"]["maxItems"],
        crate::modules::developer_workflows::MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
    );
    assert_eq!(
        schemas["AcceptedWorkloadProfileRevisionList"]["x-a3s-canonical-order"],
        json!(["revisionNumber", "workloadProfileRevisionId"])
    );
    assert_eq!(
        schemas["WorkloadSecretTarget"]["discriminator"]["propertyName"],
        "kind"
    );
    assert_eq!(
        schemas["WorkloadProcess"]["properties"]["command"]["maxItems"],
        crate::modules::developer_workflows::MAX_WORKLOAD_PROCESS_COMMANDS
    );
    assert_eq!(
        schemas["WorkloadProfileResources"]["properties"]["executionTimeoutMs"]["maximum"],
        crate::modules::developer_workflows::MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS
    );
    assert_eq!(
        schemas["ScheduledTaskSchedule"]["properties"]["maximumConcurrency"]["maximum"],
        crate::modules::developer_workflows::MAX_WORKLOAD_SCHEDULE_CONCURRENCY
    );
    for (success_schema, data_schema) in [
        (
            "AcceptedWorkloadProfileRevisionSuccessResponse",
            "AcceptedWorkloadProfileRevision",
        ),
        (
            "AcceptedWorkloadProfileRevisionListSuccessResponse",
            "AcceptedWorkloadProfileRevisionList",
        ),
        (
            "WorkloadProfileMutationSuccessResponse",
            "WorkloadProfileMutation",
        ),
    ] {
        assert_eq!(
            schemas[success_schema]["allOf"][0]["properties"]["data"]["$ref"],
            format!("#/components/schemas/{data_schema}")
        );
    }

    let profile_schemas = serde_json::to_string(&json!({
        "profile": schemas["WorkloadProfileSpec"],
        "secret": schemas["WorkloadSecretBinding"],
        "accepted": schemas["AcceptedWorkloadProfileRevision"],
    }))
    .map_err(|error| BootError::Internal(error.to_string()))?;
    for forbidden in [
        "secretValue",
        "sourceBytes",
        "checkoutPath",
        "workloadState",
        "executionState",
        "routeState",
        "schedulerState",
    ] {
        assert!(
            !profile_schemas.contains(forbidden),
            "WorkloadProfile schemas must not expose {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn workload_profile_routes_use_one_typed_revision_contract() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let collection_path = full_route(WORKLOAD_PROFILE_COLLECTION_ROUTE);
    let current_path = full_route(WORKLOAD_PROFILE_ITEM_ROUTE);
    let revisions_path = full_route(WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE);
    let revision_path = full_route(WORKLOAD_PROFILE_REVISION_ITEM_ROUTE);
    let collection = &document["paths"][collection_path]["post"];
    let current = &document["paths"][current_path]["get"];
    let revisions = &document["paths"][revisions_path]["get"];
    let revision = &document["paths"][revision_path]["get"];

    for operation in [collection, current, revisions, revision] {
        assert_eq!(operation["tags"], json!(["Developer Workflows"]));
    }
    assert_eq!(
        collection["requestBody"]["content"]["application/json"]["schema"]["required"],
        json!(["buildPlanId", "profileAcl"])
    );
    assert_eq!(
        collection["requestBody"]["content"]["application/json"]["schema"]["properties"]
            ["profileAcl"]["maxLength"],
        crate::modules::developer_workflows::WORKLOAD_PROFILE_MAX_ACL_BYTES
    );
    assert!(has_parameter(collection, "idempotency-key"));
    let limit = parameter(revisions, "limit")?;
    assert_eq!(
        limit["schema"]["default"],
        crate::modules::developer_workflows::DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
    );
    assert_eq!(
        limit["schema"]["maximum"],
        crate::modules::developer_workflows::MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
    );
    for operation in [current, revision] {
        assert_eq!(
            operation["responses"]["200"]["$ref"],
            "#/components/responses/AcceptedWorkloadProfileRevisionSuccess200"
        );
    }
    assert_eq!(
        revisions["responses"]["200"]["$ref"],
        "#/components/responses/AcceptedWorkloadProfileRevisionListSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            collection["responses"][status]["$ref"],
            format!("#/components/responses/WorkloadProfileMutationSuccess{status}")
        );
    }
    assert!(collection["responses"]["413"].is_object());
    assert!(collection["responses"]["415"].is_object());
    Ok(())
}

#[test]
fn preview_management_schemas_are_closed_bounded_revisioned_and_acl_only() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    for name in [
        "PreviewGitRepository",
        "PreviewQuota",
        "PullRequestPreviewPolicy",
        "AcceptedPullRequestPreviewPolicyRevision",
        "PullRequestPreviewPolicyMutation",
        "PullRequestPreview",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["AcceptedPullRequestPreviewPolicyRevision"]["properties"]["contractSchema"]["enum"],
        json!([crate::modules::developer_workflows::PULL_REQUEST_PREVIEW_POLICY_SCHEMA])
    );
    assert_eq!(
        schemas["AcceptedPullRequestPreviewPolicyRevision"]["properties"]["contractAcl"]
            ["maxLength"],
        crate::modules::developer_workflows::PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES
    );
    assert_eq!(
        schemas["AcceptedPullRequestPreviewPolicyRevisionList"]["maxItems"],
        crate::modules::developer_workflows::MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT
    );
    assert_eq!(
        schemas["AcceptedPullRequestPreviewPolicyRevisionList"]["x-a3s-canonical-order"],
        json!(["revisionNumber", "pullRequestPreviewPolicyRevisionId"])
    );
    for field in [
        "installationId",
        "revisionNumber",
        "pullRequestId",
        "pullRequestNumber",
        "policyRevisionNumber",
        "aggregateVersion",
    ] {
        let schema = if field == "installationId" {
            &schemas["PullRequestPreviewPolicy"]["properties"][field]
        } else if field == "revisionNumber" {
            &schemas["AcceptedPullRequestPreviewPolicyRevision"]["properties"][field]
        } else {
            &schemas["PullRequestPreview"]["properties"][field]
        };
        assert_eq!(
            schema["maximum"],
            crate::modules::developer_workflows::MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER,
            "{field} must remain portable"
        );
    }
    for (success_schema, data_schema) in [
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
    ] {
        assert_eq!(
            schemas[success_schema]["allOf"][0]["properties"]["data"]["$ref"],
            format!("#/components/schemas/{data_schema}")
        );
    }

    let encoded = serde_json::to_string(&schema_without_documentation(&json!({
        "repository": schemas["PreviewGitRepository"],
        "policy": schemas["PullRequestPreviewPolicy"],
        "accepted": schemas["AcceptedPullRequestPreviewPolicyRevision"],
        "preview": schemas["PullRequestPreview"],
    })))
    .map_err(|error| BootError::Internal(error.to_string()))?;
    for forbidden in [
        "webhookSecret",
        "signature",
        "deliveryBody",
        "credential",
        "providerToken",
        "checkoutPath",
        "buildRunId",
        "routeId",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "Preview schemas must not expose {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn preview_management_routes_use_one_typed_public_contract() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let collection =
        &document["paths"][full_route(PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE)]["post"];
    let current = &document["paths"][full_route(PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE)]["get"];
    let revisions = &document["paths"]
        [full_route(PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE)]["get"];
    let revision =
        &document["paths"][full_route(PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE)]["get"];
    let preview = &document["paths"][full_route(PULL_REQUEST_PREVIEW_ITEM_ROUTE)]["get"];

    for operation in [collection, current, revisions, revision, preview] {
        assert_eq!(operation["tags"], json!(["Developer Workflows"]));
    }
    let request = &collection["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(
        request["required"],
        json!(["sourceSubscriptionId", "policyAcl"])
    );
    assert_eq!(request["additionalProperties"], false);
    assert_eq!(
        request["properties"]["policyAcl"]["maxLength"],
        crate::modules::developer_workflows::PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES
    );
    assert!(has_parameter(collection, "idempotency-key"));
    let limit = parameter(revisions, "limit")?;
    assert_eq!(
        limit["schema"]["default"],
        crate::modules::developer_workflows::DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT
    );
    assert_eq!(
        limit["schema"]["maximum"],
        crate::modules::developer_workflows::MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT
    );
    let pull_request_id = parameter(preview, "pull_request_id")?;
    assert_eq!(pull_request_id["schema"]["type"], "integer");
    assert_eq!(
        pull_request_id["schema"]["maximum"],
        crate::modules::developer_workflows::MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER
    );
    for operation in [current, revision] {
        assert_eq!(
            operation["responses"]["200"]["$ref"],
            "#/components/responses/AcceptedPullRequestPreviewPolicyRevisionSuccess200"
        );
    }
    assert_eq!(
        revisions["responses"]["200"]["$ref"],
        "#/components/responses/AcceptedPullRequestPreviewPolicyRevisionListSuccess200"
    );
    assert_eq!(
        preview["responses"]["200"]["$ref"],
        "#/components/responses/PullRequestPreviewSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            collection["responses"][status]["$ref"],
            format!("#/components/responses/PullRequestPreviewPolicyMutationSuccess{status}")
        );
    }
    assert!(collection["responses"]["413"].is_object());
    assert!(collection["responses"]["415"].is_object());
    Ok(())
}

fn has_parameter(operation: &serde_json::Value, name: &str) -> bool {
    operation["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|value| value["name"] == name))
}

fn parameter<'a>(operation: &'a serde_json::Value, name: &str) -> Result<&'a serde_json::Value> {
    operation["parameters"]
        .as_array()
        .and_then(|parameters| parameters.iter().find(|value| value["name"] == name))
        .ok_or_else(|| BootError::Internal(format!("OpenAPI parameter {name} is missing")))
}

fn schema_without_documentation(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(entries) => serde_json::Value::Object(
            entries
                .iter()
                .filter(|(name, _)| !matches!(name.as_str(), "description" | "example"))
                .map(|(name, value)| (name.clone(), schema_without_documentation(value)))
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(schema_without_documentation).collect())
        }
        value => value.clone(),
    }
}
