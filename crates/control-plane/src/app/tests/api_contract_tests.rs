use super::*;
use crate::presentation::{
    generate_openapi_contract, API_CONTRACT_VERSION_HEADER, API_MAJOR_VERSION,
    MINIMUM_DEPRECATION_DAYS, OPENAPI_CONTRACT_VERSION, OPENAPI_PUBLIC_PATH,
};
use std::collections::BTreeSet;

const OPENAPI_SOURCE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi/v1.json");

#[tokio::test]
async fn openapi_contract_is_public_raw_and_versioned() -> Result<()> {
    let app = contract_test_application()?;
    let response = app
        .call(BootRequest::new(HttpMethod::Get, OPENAPI_PUBLIC_PATH))
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(
        response.header(API_CONTRACT_VERSION_HEADER),
        Some(OPENAPI_CONTRACT_VERSION)
    );
    assert!(response
        .header("x-request-id")
        .is_some_and(|value| Uuid::parse_str(value).is_ok()));
    assert_eq!(
        response.header("cache-control"),
        Some("public, max-age=300")
    );

    let document = response_json(&response)?;
    assert_eq!(document["openapi"], "3.0.3");
    assert_eq!(document["info"]["version"], OPENAPI_CONTRACT_VERSION);
    assert_eq!(document["x-a3s-api-major-version"], API_MAJOR_VERSION);
    assert_eq!(
        document["x-a3s-minimum-deprecation-days"],
        MINIMUM_DEPRECATION_DAYS
    );
    assert!(document.get("data").is_none());
    assert!(document["paths"]
        .as_object()
        .is_some_and(|paths| !paths.is_empty()));
    Ok(())
}

#[test]
fn committed_openapi_snapshot_matches_the_resolved_route_contract() -> Result<()> {
    let app = contract_test_application()?;
    let generated = generate_openapi_contract(&app)?;
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&generated)
            .map_err(|error| BootError::Internal(error.to_string()))?
    );

    if std::env::var_os("A3S_CLOUD_UPDATE_OPENAPI").is_some() {
        std::fs::write(OPENAPI_SOURCE_PATH, rendered)
            .map_err(|error| BootError::Internal(error.to_string()))?;
        return Ok(());
    }

    let committed: Value = serde_json::from_str(include_str!("../../../../../openapi/v1.json"))
        .map_err(|error| BootError::Internal(error.to_string()))?;
    assert_eq!(generated, committed);
    Ok(())
}

#[test]
fn generated_openapi_operations_have_stable_ids_security_and_envelopes() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let paths = document["paths"]
        .as_object()
        .ok_or_else(|| BootError::Internal("OpenAPI paths are missing".into()))?;
    let mut operation_ids = BTreeSet::new();
    let mut operation_count = 0;

    for (path, path_item) in paths {
        let path_item = path_item
            .as_object()
            .ok_or_else(|| BootError::Internal(format!("OpenAPI path `{path}` is invalid")))?;
        for method in ["delete", "get", "patch", "post", "put"] {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            operation_count += 1;
            let operation_id = operation["operationId"].as_str().ok_or_else(|| {
                BootError::Internal(format!("OpenAPI operation `{method} {path}` has no ID"))
            })?;
            assert!(operation_ids.insert(operation_id.to_owned()));
            assert!(operation["tags"]
                .as_array()
                .is_some_and(|tags| !tags.is_empty()));
            assert!(operation["security"].is_array());
            assert!(operation["responses"].get("500").is_some());
        }
    }

    assert!(
        operation_count >= 50,
        "unexpectedly small public REST surface"
    );
    assert_eq!(document["paths"]["/platform"]["get"]["security"], json!([]));
    assert_eq!(
        document["paths"]["/organizations"]["get"]["security"],
        json!([{ "bearerAuth": [] }])
    );
    assert!(document["paths"]["/organizations"]["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let log_stream = &document["paths"]
        ["/organizations/{organization_id}/build-runs/{build_run_id}/logs/stream"]["get"];
    assert_eq!(
        log_stream["responses"]["200"]["$ref"],
        "#/components/responses/SseSuccess200"
    );
    assert!(log_stream["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "last-event-id" && parameter["in"] == "header"
        })));
    assert!(document["paths"]
        .get("/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads")
        .and_then(|path| path.get("post"))
        .and_then(|operation| operation.get("requestBody"))
        .and_then(|body| body.get("content"))
        .and_then(|content| content.get("application/vnd.a3s.acl"))
        .is_some());
    for path in [
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/assets/{asset_id}/releases/{asset_release_id}/workloads",
        "/organizations/{organization_id}/workloads/{workload_id}/assets/{asset_id}/releases/{asset_release_id}/deployments",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Workloads"]));
        assert!(operation["requestBody"]["content"]["application/json"].is_object());
        assert!(operation["requestBody"]["content"]["application/vnd.a3s.acl"].is_object());
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["responses"]["202"].is_object());
    }
    let executions = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"];
    assert!(executions["get"].is_object());
    assert!(executions["post"]["requestBody"]["content"]["application/json"].is_object());
    assert!(executions["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let execution =
        &document["paths"]["/organizations/{organization_id}/executions/{execution_id}"];
    assert!(execution["get"].is_object());
    assert!(execution["delete"].is_object());

    let asset_git = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/git/info/refs"]["get"];
    assert_eq!(asset_git["tags"], json!(["Assets"]));
    assert!(asset_git["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "service"
                && parameter["in"] == "query"
                && parameter["required"] == true
                && parameter["schema"]["enum"] == json!(["git-upload-pack", "git-receive-pack"])
        })));
    assert_eq!(
        asset_git["responses"]["200"]["$ref"],
        "#/components/responses/AssetGitAdvertisementSuccess200"
    );

    let receive_pack = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/git/git-receive-pack"]["post"];
    assert!(
        receive_pack["requestBody"]["content"]["application/x-git-receive-pack-request"]
            .is_object()
    );
    assert!(receive_pack["requestBody"]["content"]
        .get("application/json")
        .is_none());
    assert!(receive_pack["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters
            .iter()
            .all(|parameter| parameter["name"] != "idempotency-key")));
    assert_eq!(
        receive_pack["responses"]["200"]["$ref"],
        "#/components/responses/AssetGitReceivePackSuccess200"
    );
    assert_eq!(
        receive_pack["responses"]["413"]["$ref"],
        "#/components/responses/Error413"
    );
    assert_eq!(
        receive_pack["responses"]["415"]["$ref"],
        "#/components/responses/Error415"
    );

    let assets = &document["paths"]["/organizations/{organization_id}/assets"];
    assert_eq!(assets["get"]["tags"], json!(["Assets"]));
    assert_eq!(assets["post"]["tags"], json!(["Assets"]));
    assert!(assets["post"]["responses"]["201"].is_object());
    assert!(assets["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key" && parameter["in"] == "header"
        })));
    let archive =
        &document["paths"]["/organizations/{organization_id}/assets/{asset_id}/archive"]["post"];
    assert!(archive.get("requestBody").is_none());
    let releases =
        &document["paths"]["/organizations/{organization_id}/assets/{asset_id}/releases"];
    assert!(releases["get"].is_object());
    assert!(releases["post"]["responses"]["201"].is_object());
    let selection = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/release-selection"]["get"];
    assert!(selection["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "version"
                && parameter["in"] == "query"
                && parameter["required"] == false
                && parameter["schema"]["type"] == "string"
        })));
    let yank = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank"]
        ["post"];
    assert!(yank.get("requestBody").is_none());
    Ok(())
}

#[tokio::test]
async fn normal_api_responses_advertise_the_contract_version() -> Result<()> {
    let app = contract_test_application()?;
    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/api/v1/platform"))
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header(API_CONTRACT_VERSION_HEADER),
        Some(OPENAPI_CONTRACT_VERSION)
    );
    Ok(())
}

fn contract_test_application() -> Result<BootApplication> {
    build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )
}
