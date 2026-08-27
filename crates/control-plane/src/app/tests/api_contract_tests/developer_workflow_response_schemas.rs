use super::*;
use crate::modules::developer_workflows::{
    BUILD_PLAN_COLLECTION_ROUTE, BUILD_PLAN_DETECTION_ROUTE, BUILD_PLAN_ITEM_ROUTE,
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX,
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
