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
    let mcp_credentials = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials"];
    assert_eq!(mcp_credentials["post"]["tags"], json!(["Edge"]));
    assert_eq!(
        mcp_credentials["post"]["responses"]["201"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialDeliverySuccess201"
    );
    assert_eq!(
        mcp_credentials["post"]["responses"]["200"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialDeliverySuccess200"
    );
    assert_eq!(
        mcp_credentials["get"]["responses"]["200"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialListSuccess200"
    );
    assert_eq!(
        mcp_credentials["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/McpCredentialExpiryRequest"
    );
    let mcp_credential = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials/{credential_id}"];
    assert_eq!(
        mcp_credential["get"]["responses"]["200"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialSuccess200"
    );
    assert_eq!(
        mcp_credential["delete"]["responses"]["200"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialMutationSuccess200"
    );
    let mcp_rotate = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials/{credential_id}/rotate"];
    assert_eq!(
        mcp_rotate["post"]["responses"]["200"]["$ref"],
        "#/components/responses/SensitiveMcpCredentialDeliverySuccess200"
    );
    assert_eq!(
        mcp_rotate["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/McpCredentialExpiryRequest"
    );
    let mcp_schema = &document["components"]["schemas"]["McpCredential"];
    assert_eq!(mcp_schema["additionalProperties"], false);
    assert!(mcp_schema["properties"].get("secret").is_none());
    assert!(mcp_schema["properties"].get("verifier").is_none());
    assert_eq!(
        document["components"]["schemas"]["McpCredentialDelivery"]["properties"]["secret"]
            ["readOnly"],
        true
    );
    assert_eq!(
        document["components"]["responses"]["SensitiveMcpCredentialDeliverySuccess201"]["headers"]
            ["cache-control"]["schema"]["enum"],
        json!(["no-store"])
    );
    assert_eq!(
        document["components"]["responses"]["SensitiveError409"]["headers"]["pragma"]["schema"]
            ["enum"],
        json!(["no-cache"])
    );
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
